import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Folder } from "@/components/icons";
import {
  settingsClearNotesDir,
  settingsRead,
  settingsRevealNotes,
  settingsSetNotesDir,
  type Settings as SettingsData,
} from "@/lib/ipc";

export default function Settings() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setSettings(await settingsRead());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function chooseFolder() {
    setError(null);
    setNote(null);
    // The OS picker is the only way a path gets in — the webview never names
    // a directory itself, and Rust re-checks that it exists and is writable.
    const picked = await open({ directory: true, multiple: false, title: "Where should notes live?" });
    if (typeof picked !== "string") return;

    setBusy(true);
    try {
      const copied = await settingsSetNotesDir(picked);
      setNote(
        copied > 0
          ? `Moved ${copied} ${copied === 1 ? "note" : "notes"} into that folder.`
          : "Using that folder. Nothing needed copying.",
      );
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function useAppStorage() {
    setError(null);
    setBusy(true);
    try {
      await settingsClearNotesDir();
      setNote("Back to app storage. Your folder was left exactly as it is.");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const folder = settings?.notesDir ?? null;

  return (
    <>
      <header className="topbar" data-tauri-drag-region>
        <h1 className="page-title">Settings</h1>
      </header>

      <div className="content settings">
        {error && <p className="error">{error}</p>}

        <section className="card">
          <div className="setting-head">
            <div>
              <h2 className="setting-title">Where notes are stored</h2>
              <p className="hint">
                One <code>YYYY-MM-DD.md</code> file per day. Point this at a folder your backup or
                sync already covers — iCloud Drive, Dropbox, an Obsidian vault — and your notes are
                plain markdown you can read anywhere.
              </p>
            </div>
          </div>

          <div className="storage">
            <div className="storage-where">
              <Folder />
              <span className="path">{folder ?? settings?.dbPath ?? "—"}</span>
            </div>
            <span className={folder ? "chip ok" : "chip warn"}>
              {folder ? "folder" : "app storage"}
            </span>
          </div>

          {!folder && (
            <p className="hint">
              App storage lives inside the application's own data directory. It works, but nothing
              backs it up unless Time Machine happens to be running.
            </p>
          )}

          {note && <p className="hint accent-note">{note}</p>}

          <div className="row">
            <button className="btn primary" onClick={chooseFolder} disabled={busy}>
              {folder ? "Choose a different folder" : "Choose a folder"}
            </button>
            {folder && (
              <button className="btn" onClick={useAppStorage} disabled={busy}>
                Use app storage
              </button>
            )}
            <button className="btn quiet" onClick={() => void settingsRevealNotes()}>
              Reveal in Finder
            </button>
          </div>

          <p className="hint">
            Switching copies your notes across. Files already in the folder are never overwritten —
            if a day exists in both places, the folder wins.
          </p>
        </section>

        <section className="card">
          <h2 className="setting-title">About</h2>
          <dl className="facts">
            <dt>Version</dt>
            <dd>{settings?.version ?? "—"}</dd>
            <dt>Account</dt>
            <dd>{settings?.account ? `@${settings.account.login}` : "not connected"}</dd>
            <dt>API</dt>
            <dd>{settings?.apiBase ?? "—"}</dd>
            <dt>Database</dt>
            <dd>{settings?.dbPath ?? "—"}</dd>
            <dt>Schema</dt>
            <dd>v{settings?.schemaVersion ?? "—"}</dd>
          </dl>
        </section>
      </div>
    </>
  );
}
