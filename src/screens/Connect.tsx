import { useState } from "react";
import { authBegin, authCancel, openExternal, type AppInfo, type DeviceStart } from "@/lib/ipc";

type Props = {
  info: AppInfo | null;
  error: string | null;
  onError: (message: string | null) => void;
};

export default function Connect({ info, error, onError }: Props) {
  const [start, setStart] = useState<DeviceStart | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const missingClientId = info != null && !info.clientIdConfigured;

  async function begin() {
    setBusy(true);
    onError(null);
    try {
      const s = await authBegin();
      setStart(s);
      await openExternal(s.verificationUri);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function cancel() {
    await authCancel();
    setStart(null);
  }

  async function copyCode() {
    if (!start) return;
    await navigator.clipboard.writeText(start.userCode);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  }

  return (
    <div className="pane">
      <h1 className="mark">
        Everything that <em>you want</em>,
        <br />
        in one place.
      </h1>

      {error && <p className="error">{error}</p>}

      {missingClientId ? (
        <div className="card">
          <span className="eyebrow">Setup needed</span>
          <p style={{ margin: 0, color: "var(--ink-soft)" }}>
            Workbench needs a GitHub OAuth app client ID before it can sign you in. Create one with device
            flow enabled, then set <code>COZY_GITHUB_CLIENT_ID</code> and restart.
          </p>
          <button className="btn" onClick={() => openExternal("https://github.com/settings/developers")}>
            Open GitHub developer settings
          </button>
        </div>
      ) : start ? (
        <div className="card">
          <span className="eyebrow">Enter this code on GitHub</span>
          <div className="usercode" onClick={copyCode} title="Click to copy">
            {start.userCode}
          </div>
          <div className="status">
            <span className="pip waiting" />
            {copied ? "Copied to clipboard" : "Waiting for you to approve…"}
          </div>
          <div className="row">
            <button className="btn" onClick={() => openExternal(start.verificationUri)}>
              Reopen GitHub
            </button>
            <button className="btn quiet" onClick={cancel}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="row center">
          <button className="btn primary" onClick={begin} disabled={busy}>
            {busy ? "Starting…" : "Connect your GitHub account"}
          </button>
        </div>
      )}

      <p className="hint">
        Your token is stored in your system keychain and never leaves this machine.
      </p>
    </div>
  );
}
