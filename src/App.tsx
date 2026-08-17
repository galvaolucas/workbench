import { useEffect, useState } from "react";
import { appInfo, authStatus, onAuthCompleted, onAuthFailed, type Account, type AppInfo } from "@/lib/ipc";
import { Lanes, Notes, Sunrise } from "@/components/icons";
import Connect from "@/screens/Connect";
import Desk from "@/screens/Desk";
import Today from "@/screens/Today";

type Tab = "today" | "desk";

export default function App() {
  const [account, setAccount] = useState<Account | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Notes need no GitHub account, so the app opens on something usable
  // whether or not one has ever been connected.
  const [tab, setTab] = useState<Tab>("today");

  useEffect(() => {
    let alive = true;

    Promise.all([authStatus(), appInfo()])
      .then(([acct, meta]) => {
        if (!alive) return;
        setAccount(acct);
        setInfo(meta);
      })
      .catch((e) => alive && setError(String(e)))
      .finally(() => alive && setReady(true));

    // Device-flow polling happens in Rust, so the result arrives as an event.
    const unlisteners = [
      onAuthCompleted((acct) => {
        setAccount(acct);
        setError(null);
      }),
      onAuthFailed((message) => setError(message)),
    ];

    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      if (e.key === "1") setTab("today");
      if (e.key === "2") setTab("desk");
    };
    window.addEventListener("keydown", onKey);

    return () => {
      alive = false;
      window.removeEventListener("keydown", onKey);
      unlisteners.forEach((p) => p.then((off) => off()));
    };
  }, []);

  return (
    <div className="shell">
      <nav className="rail" data-tauri-drag-region>
        <Sunrise size={28} className="rail-mark" />
        <button
          className={tab === "today" ? "rail-btn on" : "rail-btn"}
          onClick={() => setTab("today")}
          title="Today (⌘1)"
          aria-label="Today"
        >
          <Notes />
        </button>
        <button
          className={tab === "desk" ? "rail-btn on" : "rail-btn"}
          onClick={() => setTab("desk")}
          title="Desk (⌘2)"
          aria-label="Desk"
        >
          <Lanes />
        </button>
        <span className="rail-spacer" />
      </nav>

      {/* Both panes stay mounted: switching must never throw away a
          half-typed note or refetch the Desk. */}
      <div className="main">
        <div className={tab === "today" ? "pane" : "pane hidden"}>
          <Today />
        </div>
        <div className={tab === "desk" ? "pane" : "pane hidden"}>
          {!ready ? null : account ? (
            <Desk account={account} onSignedOut={() => setAccount(null)} />
          ) : (
            <Connect info={info} error={error} onError={setError} />
          )}
        </div>
      </div>
    </div>
  );
}
