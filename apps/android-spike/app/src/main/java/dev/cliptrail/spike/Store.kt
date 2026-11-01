package dev.cliptrail.spike

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One capture attempt as shown in the log. [preview] is plaintext in memory, encrypted on disk. */
data class Attempt(
    val at: Long,
    val signal: String,
    val verdict: String,
    val pkg: String?,
    val preview: String?,
    val kind: String?,
    val latencyMs: Long,
    val ageMs: Long,
    val hash: String?,
    val note: String,
)

/** A distinct accessibility event shape (type + package + class) seen on this phone. */
data class Signature(
    val type: String,
    val pkg: String,
    val cls: String,
    val count: Long,
    val lastText: String?,
    val signal: String?,
    val lastAt: Long,
)

/**
 * Tiny local log (survives restarts). Never leaves the device: the app has no INTERNET permission.
 * Clip previews and toast texts are AES-GCM encrypted with a non-exportable Android Keystore key
 * (AGENTS.md: encrypt clip content before persisting).
 */
class Store(context: Context) : SQLiteOpenHelper(context, "spike.db", null, 1) {

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE kv (key TEXT PRIMARY KEY, ival INTEGER NOT NULL DEFAULT 0, sval TEXT)")
        db.execSQL(
            "CREATE TABLE attempts (id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER, signal TEXT, verdict TEXT, " +
                "pkg TEXT, preview TEXT, kind TEXT, latency_ms INTEGER, age_ms INTEGER, hash TEXT, note TEXT)",
        )
        db.execSQL(
            "CREATE TABLE signatures (sig TEXT PRIMARY KEY, type TEXT, pkg TEXT, cls TEXT, count INTEGER, " +
                "last_text TEXT, signal TEXT, last_at INTEGER)",
        )
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) = Unit

    // ---- key/value counters -------------------------------------------------------------------

    // No UPSERT: Android 10 (API 29) ships SQLite 3.22, older than ON CONFLICT DO UPDATE.
    fun inc(key: String, by: Long = 1) {
        val db = writableDatabase
        val updated = db.compileStatement("UPDATE kv SET ival = ival + ? WHERE key = ?").use { st ->
            st.bindLong(1, by)
            st.bindString(2, key)
            st.executeUpdateDelete()
        }
        if (updated == 0) db.insert("kv", null, ContentValues().apply { put("key", key); put("ival", by) })
    }

    fun putLong(key: String, value: Long) {
        writableDatabase.insertWithOnConflict(
            "kv", null, ContentValues().apply { put("key", key); put("ival", value) },
            SQLiteDatabase.CONFLICT_REPLACE,
        )
    }

    fun setMax(key: String, value: Long) {
        if (value > long(key)) putLong(key, value)
    }

    fun long(key: String): Long =
        readableDatabase.rawQuery("SELECT ival FROM kv WHERE key = ?", arrayOf(key)).use { c ->
            if (c.moveToFirst()) c.getLong(0) else 0L
        }

    fun putText(key: String, value: String) {
        writableDatabase.insertWithOnConflict(
            "kv", null, ContentValues().apply { put("key", key); put("sval", value) },
            SQLiteDatabase.CONFLICT_REPLACE,
        )
    }

    fun text(key: String): String? =
        readableDatabase.rawQuery("SELECT sval FROM kv WHERE key = ?", arrayOf(key)).use { c ->
            if (c.moveToFirst() && !c.isNull(0)) c.getString(0) else null
        }

    fun counters(): Map<String, Long> =
        readableDatabase.rawQuery("SELECT key, ival FROM kv WHERE sval IS NULL", null).use { c ->
            buildMap { while (c.moveToNext()) put(c.getString(0), c.getLong(1)) }
        }

    // ---- attempts -----------------------------------------------------------------------------

    fun addAttempt(a: Attempt) {
        val db = writableDatabase
        db.insert(
            "attempts", null,
            ContentValues().apply {
                put("at", a.at); put("signal", a.signal); put("verdict", a.verdict); put("pkg", a.pkg)
                put("preview", a.preview?.let(Crypto::encrypt)); put("kind", a.kind)
                put("latency_ms", a.latencyMs); put("age_ms", a.ageMs); put("hash", a.hash); put("note", a.note)
            },
        )
        db.execSQL("DELETE FROM attempts WHERE id <= (SELECT MAX(id) FROM attempts) - $MAX_ATTEMPTS")
    }

    fun attempts(limit: Int): List<Attempt> =
        readableDatabase.rawQuery(
            "SELECT at, signal, verdict, pkg, preview, kind, latency_ms, age_ms, hash, note " +
                "FROM attempts ORDER BY id DESC LIMIT $limit",
            null,
        ).use { c ->
            buildList {
                while (c.moveToNext()) {
                    add(
                        Attempt(
                            at = c.getLong(0), signal = c.getString(1), verdict = c.getString(2),
                            pkg = c.getStringOrNull(3), preview = c.getStringOrNull(4)?.let(Crypto::decrypt),
                            kind = c.getStringOrNull(5), latencyMs = c.getLong(6), ageMs = c.getLong(7),
                            hash = c.getStringOrNull(8), note = c.getStringOrNull(9).orEmpty(),
                        ),
                    )
                }
            }
        }

    /** Hashes of accepted clips, oldest first, to restore the dedup set after a restart. */
    fun acceptedHashes(limit: Int): List<String> =
        readableDatabase.rawQuery(
            "SELECT hash FROM attempts WHERE verdict = ? AND hash IS NOT NULL ORDER BY id DESC LIMIT $limit",
            arrayOf(Verdict.ACCEPTED.name),
        ).use { c -> buildList { while (c.moveToNext()) add(c.getString(0)) } }.asReversed()

    // ---- event signatures ---------------------------------------------------------------------

    fun recordSignature(type: String, pkg: String, cls: String, text: String?, signal: String?, at: Long) {
        val sig = "$type|$pkg|$cls"
        val db = writableDatabase
        val enc = text?.let(Crypto::encrypt)
        val updated = db.compileStatement(
            "UPDATE signatures SET count = count + 1, last_at = ?, last_text = COALESCE(?, last_text), " +
                "signal = COALESCE(?, signal) WHERE sig = ?",
        ).use { st ->
            st.bindLong(1, at)
            if (enc != null) st.bindString(2, enc) else st.bindNull(2)
            if (signal != null) st.bindString(3, signal) else st.bindNull(3)
            st.bindString(4, sig)
            st.executeUpdateDelete()
        }
        if (updated == 0) {
            db.insert(
                "signatures", null,
                ContentValues().apply {
                    put("sig", sig); put("type", type); put("pkg", pkg); put("cls", cls); put("count", 1)
                    put("last_text", enc); put("signal", signal); put("last_at", at)
                },
            )
            db.execSQL(
                "DELETE FROM signatures WHERE sig IN (SELECT sig FROM signatures ORDER BY last_at DESC " +
                    "LIMIT -1 OFFSET $MAX_SIGNATURES)",
            )
        }
    }

    fun signatures(limit: Int): List<Signature> =
        readableDatabase.rawQuery(
            "SELECT type, pkg, cls, count, last_text, signal, last_at FROM signatures " +
                "ORDER BY (signal IS NULL), last_at DESC LIMIT $limit",
            null,
        ).use { c ->
            buildList {
                while (c.moveToNext()) {
                    add(
                        Signature(
                            c.getString(0), c.getString(1), c.getString(2), c.getLong(3),
                            c.getStringOrNull(4)?.let(Crypto::decrypt), c.getStringOrNull(5), c.getLong(6),
                        ),
                    )
                }
            }
        }

    /** Wipe counters, attempts and signatures; keep the hash salt. */
    fun resetAll() {
        val db = writableDatabase
        db.execSQL("DELETE FROM attempts")
        db.execSQL("DELETE FROM signatures")
        db.execSQL("DELETE FROM kv WHERE key != 'salt'")
    }

    private fun android.database.Cursor.getStringOrNull(i: Int): String? = if (isNull(i)) null else getString(i)

    companion object {
        private const val MAX_ATTEMPTS = 500
        private const val MAX_SIGNATURES = 300
    }
}

/** AES-256-GCM with a key that never leaves the Android Keystore. */
object Crypto {
    private const val ALIAS = "cliptrail_spike_log"
    private const val TRANSFORM = "AES/GCM/NoPadding"
    private const val IV_LEN = 12

    private val key: SecretKey by lazy {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey) ?: KeyGenerator
            .getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            .apply {
                init(
                    KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256)
                        .build(),
                )
            }
            .generateKey()
    }

    fun encrypt(plain: String): String? = try {
        val c = Cipher.getInstance(TRANSFORM)
        c.init(Cipher.ENCRYPT_MODE, key)
        Base64.encodeToString(c.iv + c.doFinal(plain.toByteArray()), Base64.NO_WRAP)
    } catch (_: Exception) {
        null // never fall back to storing plaintext
    }

    fun decrypt(blob: String): String? = try {
        val b = Base64.decode(blob, Base64.NO_WRAP)
        val c = Cipher.getInstance(TRANSFORM)
        c.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, b, 0, IV_LEN))
        String(c.doFinal(b, IV_LEN, b.size - IV_LEN))
    } catch (_: Exception) {
        null
    }
}
