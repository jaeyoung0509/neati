# In-app updater (macOS) — maintainer setup

User-facing behaviour is described in the README (*Updating neati*); the trust
model is in [CODE_SIGNING_POLICY.md](../CODE_SIGNING_POLICY.md#update-archive-signing-macos).
This page is the one-time setup and the release checklist.

## One-time key setup

1. Generate the key pair on your own machine (you choose the password):

   ```bash
   pnpm tauri signer generate -w ~/.tauri/neati-updater.key
   ```

2. Store the private key and password as repository secrets:

   ```bash
   gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/neati-updater.key
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
   ```

3. Put the **public** key (`~/.tauri/neati-updater.key.pub`) into
   `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` and commit it.
4. Back up the private key offline. Without it, installed copies cannot be
   updated in place.

Until step 3 is done, the app reports *"This build has no update signing key"*
and the release job refuses to run.

## Optional: stable self-signed code identity

Create a code-signing certificate once in Keychain Access → Certificate
Assistant → Create a Certificate (Identity Type: Self-Signed Root, Certificate
Type: Code Signing), export it as `.p12`, then:

```bash
base64 -i neati-codesign.p12 | gh secret set APPLE_CERTIFICATE
gh secret set APPLE_CERTIFICATE_PASSWORD
gh secret set APPLE_SIGNING_IDENTITY   # the certificate's common name
```

This keeps Full Disk Access across updates. It does not remove the Gatekeeper
warning and is not notarization.

## How a release reaches installed copies

1. The macOS job builds `neati.app.tar.gz` and its `.sig` with
   `.github/tauri.updater-ci.json`.
2. The publisher writes `latest.json` (`scripts/updater_manifest.cjs`) and
   attaches it, the archive and the signature to the versioned release.
3. After that release exists, it replaces `latest.json` on the rolling
   `updater-manifest` prerelease, which is the endpoint in `tauri.conf.json`.

The app checks only when the person presses **Check now** or once per launch if
they enabled it. Windows builds report self-update as unavailable.
