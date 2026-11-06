# Code signing

## Windows (SignPath Foundation)

Windows installers are signed for free by [SignPath Foundation](https://signpath.org),
which signs open-source projects from CI. The release workflow already
contains the signing job (`windows-sign` in `.github/workflows/release.yml`);
it stays switched off until the `SIGNPATH_ORGANIZATION_ID` repository
variable exists, so releases keep working unsigned until then.

### How a signed release works

1. The `desktop` job builds the Windows installers with tauri-action and
   uploads them to the draft release (unsigned), as today. It also keeps a
   copy as the `windows-unsigned` workflow artifact.
2. `windows-sign` submits that artifact to SignPath and waits up to 100
   minutes. **The approver gets an email and approves the request in
   SignPath** (every release needs this manual approval).
3. The signed `-setup.exe` and `.msi` are checked with `osslsigncode`, the
   Tauri updater signatures (`.sig`) are recomputed over the signed files
   (Authenticode changes the bytes), and the installers, `.sig` files and
   `latest.json` are replaced in the draft release.
4. Check the draft as usual, then publish.

If the request is rejected or times out, the job fails and the draft keeps
the unsigned installers. Don't publish it; re-run the `windows-sign` job.

### One-time setup

1. **Apply** at <https://signpath.org/apply> with the GitHub account that
   owns the repository. Their conditions are at <https://signpath.org/terms>;
   the code signing policy they require is published at
   `site/code-signing.html` (linked from every page footer).
2. **Turn on MFA** for everyone with commit or approval rights, on GitHub
   and on SignPath.
3. Once approved, in SignPath:
   - Add the **GitHub.com trusted build system** to the organization and
     link it to the project.
   - Project slug `cliptrail`, signing policy slug `release-signing`
     (approval required, approver: the maintainer).
   - Artifact configuration slug `windows-installers`, with the XML below.
   - Create an API token for a user with *submitter* rights on the
     project and policy.
4. In GitHub → Settings → Secrets and variables → Actions:
   - Secret `SIGNPATH_API_TOKEN`: the API token.
   - Variable `SIGNPATH_ORGANIZATION_ID`: the organization id. Setting
     this variable is what turns signing on.
   - Optional variables if you chose other slugs:
     `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_SIGNING_POLICY_SLUG`,
     `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`.
5. Update `site/_src/pages.py` (`SIGNING`): replace the "has been applied
   for" callout with the date signed builds started, and drop the "not
   code-signed" notes in the install docs. Run `python site/_src/build.py`.

### Artifact configuration (`windows-installers`)

The workflow uploads a zip with `nsis/` and `msi/` folders and passes the
release version (for example `0.1.2`) as the `version` parameter. The
installers carry `ProductName` = `ClipTrail` and `ProductVersion` = the
release version, which satisfies the Foundation's metadata restrictions.

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" default-value="0.0.0" />
  </parameters>
  <zip-file>
    <directory path="nsis">
      <pe-file path="ClipTrail_*_x64-setup.exe" product-name="ClipTrail" product-version="${version}">
        <authenticode-sign />
      </pe-file>
    </directory>
    <directory path="msi">
      <msi-file path="ClipTrail_*_x64_en-US.msi">
        <authenticode-sign />
      </msi-file>
    </directory>
  </zip-file>
</artifact-configuration>
```

Check it in SignPath's artifact configuration editor when you create it;
it validates the syntax.

### Not signed (yet)

- The app's own `cliptrail.exe` inside the installer. Windows SmartScreen
  judges the downloaded installer, so signing it removes the warning.
  Signing the inner exe would need Tauri's `signCommand`, which SignPath's
  GitHub flow doesn't plug into.
- The `clipd` CLI zip.

## macOS

Not set up. It needs an Apple Developer Program membership ($99/year) and
the `APPLE_*` secrets described in `docs/release.md`.
