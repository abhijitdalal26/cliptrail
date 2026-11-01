//! JNI entry points for `dev.cliptrail.app.NativeCore` (Kotlin). They let
//! the accessibility service, Quick Settings tile, share target and
//! text-selection activity write into the clip store without the UI.
//!
//! Kotlin calls these off the main thread. Every body is panic-guarded (a
//! panic must never cross the FFI boundary). Clip text is never logged.
//!
//! Return codes of the ingest functions: 1 stored, 2 already saved (bumped),
//! 0 skipped (paused, empty, excluded app, ...), -1 store unavailable.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use clipd_core::{IngestOutcome, RawClip, RawContent, now_millis};
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong};

/// Largest image we accept from Kotlin (encoded PNG bytes).
const MAX_PNG_BYTES: usize = 40 * 1024 * 1024;
/// Largest decoded image side (Kotlin downscales before this).
const MAX_SIDE: u32 = 8192;

fn jstr(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(String::from)
}

fn code(r: Result<IngestOutcome, String>) -> jint {
    match r {
        Ok(IngestOutcome::Stored { .. }) => 1,
        Ok(IngestOutcome::Bumped { .. }) => 2,
        Ok(IngestOutcome::Skipped { .. }) => 0,
        Err(e) => {
            tracing::warn!("native ingest failed: {e}");
            -1
        }
    }
}

fn at(captured_at: jlong) -> i64 {
    if captured_at > 0 {
        captured_at
    } else {
        now_millis()
    }
}

fn source(pkg: Option<String>) -> Option<String> {
    pkg.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cliptrail_app_NativeCore_nativeInit<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    data_dir: JString<'l>,
    keys_dir: JString<'l>,
    device_name: JString<'l>,
) -> jboolean {
    super::logcat::init();
    let (Some(data), Some(keys)) = (jstr(&mut env, &data_dir), jstr(&mut env, &keys_dir)) else {
        return JNI_FALSE;
    };
    let name = jstr(&mut env, &device_name).unwrap_or_default();
    let ok = catch_unwind(AssertUnwindSafe(|| {
        let core = super::init(PathBuf::from(data), PathBuf::from(keys), name);
        core.pipeline().is_ok()
    }));
    match ok {
        Ok(true) => JNI_TRUE,
        _ => JNI_FALSE,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cliptrail_app_NativeCore_ingestText<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    text: JString<'l>,
    html: JString<'l>,
    source_pkg: JString<'l>,
    captured_at: jlong,
) -> jint {
    let Some(text) = jstr(&mut env, &text) else {
        return 0;
    };
    let html = jstr(&mut env, &html).filter(|h| !h.is_empty());
    let source_app = source(jstr(&mut env, &source_pkg));
    let raw = RawClip {
        content: RawContent::Text { text, html },
        source_app,
        captured_at: at(captured_at),
    };
    catch_unwind(AssertUnwindSafe(|| code(super::ingest(raw)))).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cliptrail_app_NativeCore_ingestImage<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    png: JByteArray<'l>,
    source_pkg: JString<'l>,
    captured_at: jlong,
) -> jint {
    let mut env = env;
    if png.is_null() {
        return 0;
    }
    let Ok(len) = env.get_array_length(&png) else {
        return 0;
    };
    if len <= 0 || len as usize > MAX_PNG_BYTES {
        tracing::info!(len, "image skipped (size)");
        return 0;
    }
    let Ok(bytes) = env.convert_byte_array(&png) else {
        return 0;
    };
    let source_app = source(jstr(&mut env, &source_pkg));
    let captured_at = at(captured_at);
    catch_unwind(AssertUnwindSafe(|| {
        let img = match image::load_from_memory_with_format(&bytes, image::ImageFormat::Png) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("couldn't decode a shared image: {e}");
                return 0;
            }
        };
        if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
            return 0;
        }
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        code(super::ingest(RawClip {
            content: RawContent::Image {
                width,
                height,
                rgba: rgba.into_raw(),
            },
            source_app,
            captured_at,
        }))
    }))
    .unwrap_or(-1)
}

/// True while the user paused capture from the app.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cliptrail_app_NativeCore_isPaused<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jboolean {
    match super::core() {
        Ok(c) if c.is_paused() => JNI_TRUE,
        _ => JNI_FALSE,
    }
}
