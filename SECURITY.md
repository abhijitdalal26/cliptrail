# Security Model

A clipboard manager sits in the most sensitive data path on your machine. This is what ClipTrail promises, in plain terms.

## What the relay can see

The sync relay stores and forwards **ciphertext only**. It can observe metadata — timestamps, device IDs, approximate payload sizes — but never clip contents. Encryption keys are derived on-device and never leave your devices.

## What the client does with sensitive clips

- Honors OS "concealed/transient" clipboard flags.
- Auto-detects OTPs, API keys, tokens, private-key headers, and card numbers by shape → skipped or auto-expiring by default.
- Ships a preset exclusion list for password managers (1Password, Bitwarden, KeePassXC, …) plus a per-app blocklist you control.
- Local database is encrypted at rest.

## Scope

Known platform limits (Android 10+ background reads, iOS sandboxing) shape what capture paths exist; see `docs/android-capture` when it lands. Bugs that break the trust model above — plaintext reaching the relay, secrets written to logs, key material leaving the device — are treated as **critical**.

## Reporting a vulnerability

Please **do not** open a public issue for security reports. Email the maintainer or use GitHub's private vulnerability reporting. We aim to acknowledge within 72 hours.

- Paste-type disclosure (someone else's history) → critical
- Relay reading clip contents → critical
- Local DB plaintext exposure → high
