# Google Drive setup

[← Back to the README](../README.md) · [All docs](README.md)

Connect Google Drive with a service account (Workspace) or your own Google account (OAuth).

## Option A: Service account + Shared Drive (Google Workspace)

Service accounts have **no Drive storage of their own**. Uploads to a regular "My Drive" folder fail with
`storageQuotaExceeded`. Use a Shared Drive:

1. In Google Cloud console, create a project, enable the **Google Drive API**, and create a **service account**.
   Under *Keys → Add key → JSON*, download the key file.
2. In Google Drive, create a **Shared Drive** (or a folder inside one) and add the service account e-mail as a member
   with the **Content manager** role.
3. Create a *Google Drive* destination with *Service account*: upload the JSON key and paste the folder link.

Advanced: with domain-wide delegation you can instead *impersonate* a Workspace user (files then count against
that user's quota).

## Option B: Google account via OAuth (personal Gmail or Workspace)

1. In Google Cloud console, enable the **Google Drive API**. Under *APIs & Services → Credentials*, create an
   **OAuth client ID** of type **Web application**.
2. Add the authorized redirect URI shown in the destination form: `<ODB_PUBLIC_URL>/api/oauth/google/callback`
   (e.g. `http://localhost:8080/api/oauth/google/callback`).
3. On the OAuth consent screen, set the publishing status to **In production**. Apps left in *Testing* receive
   refresh tokens that expire after 7 days. An unverified app works for your own account after clicking through
   the warning.
4. Create a *Google Drive* destination with *Google account (OAuth)*, enter the client ID and secret, then click
   **Save & connect Google account**. Leave the folder empty to use `My Drive/Odoo Backups`.

Files are organised as `<folder>/<instance>/<database>/<instance>_<database>_<YYYY-MM-DD_HH-MM-SS>.zip`. The
sub-folders can be turned off per destination.
