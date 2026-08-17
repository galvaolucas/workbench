import { useCallback, useEffect, useRef, useState } from "react";
import { Refresh, SignOut } from "@/components/icons";
import { openExternal } from "@/lib/ipc";
import {
  authLogout,
  desk as readDesk,
  onDeskUpdated,
  openPullRequest,
  syncNow,
  type Account,
  type Desk as DeskData,
  type PullRequest,
} from "@/lib/ipc";

const EMPTY: DeskData = {
  needsYou: [],
  yours: [],
  watching: [],
  lastSyncedAt: null,
  visibleOrgs: [],
  orgAccessUrl: null,
};

type Props = {
  account: Account;
  onSignedOut: () => void;
};

export default function Desk({ account, onSignedOut }: Props) {
  const [data, setData] = useState<DeskData>(EMPTY);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const startedRef = useRef(false);

  const load = useCallback(async () => {
    setData(await readDesk());
  }, []);

  const sync = useCallback(async () => {
    setSyncing(true);
    setError(null);
    try {
      await syncNow();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncing(false);
    }
  }, [load]);

  useEffect(() => {
    // StrictMode mounts effects twice in development, which was firing two
    // syncs a second apart and burning double the rate limit. A ref survives
    // the remount; state would not have updated in time to guard it.
    const firstRun = !startedRef.current;
    startedRef.current = true;

    // Local state first so the window is never blank, then go to the network.
    load()
      .then(() => setLoaded(true))
      .then(() => (firstRun ? sync() : undefined))
      .catch((e) => {
        setError(String(e));
        setLoaded(true);
      });

    const off = onDeskUpdated(() => void load());
    return () => void off.then((fn) => fn());
  }, [load, sync]);

  const total = data.needsYou.length + data.yours.length + data.watching.length;

  return (
    <>
      {/* Draggable: children keep their own clicks, so the empty space
          between them behaves like a native title bar. */}
      <header className="topbar" data-tauri-drag-region>
        <h1 className="page-title">Desk</h1>
        <div className="topbar-side">
          <span className="meta">{describeSync(data.lastSyncedAt, syncing)}</span>
          <button className="icon-btn" onClick={sync} disabled={syncing} title="Refresh">
            <Refresh />
          </button>
          <span className="who-mini">
            {account.avatarUrl && <img className="avatar-sm" src={account.avatarUrl} alt="" />}
            @{account.login}
          </span>
          <button
            className="icon-btn"
            onClick={() => authLogout().then(onSignedOut)}
            title="Sign out"
          >
            <SignOut size={16} />
          </button>
        </div>
      </header>

      <div className="content">
        {error && <p className="error">{error}</p>}

        {loaded && total === 0 && !syncing ? (
          <div className="clear">
            <p className="clear-mark">Desk clear</p>
            <p className="hint">Nothing is waiting on you. Enjoy it.</p>

            {/* An empty desk and a blocked one look identical from here, so
                always say what the token can actually reach. GitHub omits
                unapproved orgs from search silently — no error, no warning. */}
            <div className="reach">
              <p className="hint">
                {data.visibleOrgs.length > 0
                  ? `Workbench can see: ${data.visibleOrgs.join(", ")}`
                  : "Workbench cannot see any organisation."}
              </p>
              {data.orgAccessUrl && (
                <button
                  className="btn"
                  onClick={() => void openExternal(data.orgAccessUrl!)}
                >
                  Organisation access
                </button>
              )}
              <p className="hint">
                Missing an org you work in? It has to approve Workbench before its pull requests
                appear here — until then GitHub returns nothing for them, without an error.
              </p>
            </div>
          </div>
        ) : (
          <div className="lanes">
            <Lane title="Needs you" hot items={data.needsYou} />
            <Lane title="Yours in flight" items={data.yours} />
            <Lane title="Watching" items={data.watching} />
          </div>
        )}
      </div>
    </>
  );
}

function Lane({ title, items, hot }: { title: string; items: PullRequest[]; hot?: boolean }) {
  return (
    <section className={hot ? "lane hot" : "lane"}>
      <div className="lane-head">
        <span className="lane-name">{title}</span>
        <span className="lane-count">{items.length}</span>
      </div>
      {items.length === 0 ? (
        <p className="lane-empty">Nothing here</p>
      ) : (
        items.map((pr) => <Card key={pr.id} pr={pr} />)
      )}
    </section>
  );
}

function Card({ pr }: { pr: PullRequest }) {
  const checks = describeChecks(pr.checksState);
  const review = describeReview(pr.reviewDecision);

  return (
    <button className="pr" onClick={() => openPullRequest(pr.id, pr.url)}>
      <span className="pr-repo">
        {pr.repo} <span className="pr-num">#{pr.number}</span>
        {pr.unread.length > 0 && <span className="pr-new" title={pr.unread.join(", ")} />}
      </span>
      <span className="pr-title">{pr.title}</span>
      <span className="pr-meta">
        {pr.author && !pr.isAuthor && <span>@{pr.author}</span>}
        {pr.isDraft && <span className="chip plain">draft</span>}
        <span className="add">+{pr.additions}</span>
        <span className="del">−{pr.deletions}</span>
        {checks && <span className={`chip ${checks.tone}`}>{checks.label}</span>}
        {review && <span className={`chip ${review.tone}`}>{review.label}</span>}
        {pr.commentCount > 0 && <span>{pr.commentCount} comments</span>}
        <span className="age">{age(pr.updatedAt)}</span>
      </span>
    </button>
  );
}

function describeChecks(state: PullRequest["checksState"]) {
  switch (state) {
    case "SUCCESS":
      return { label: "checks pass", tone: "ok" };
    case "FAILURE":
    case "ERROR":
      return { label: "checks failed", tone: "bad" };
    case "PENDING":
    case "EXPECTED":
      return { label: "checks running", tone: "warn" };
    default:
      return null;
  }
}

function describeReview(decision: PullRequest["reviewDecision"]) {
  switch (decision) {
    case "APPROVED":
      return { label: "approved", tone: "ok" };
    case "CHANGES_REQUESTED":
      return { label: "changes requested", tone: "bad" };
    case "REVIEW_REQUIRED":
      return { label: "needs review", tone: "warn" };
    default:
      return null;
  }
}

function age(epochSeconds: number) {
  const mins = Math.max(0, Math.round((Date.now() / 1000 - epochSeconds) / 60));
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.round(hours / 24)}d`;
}

function describeSync(at: number | null, syncing: boolean) {
  if (syncing) return "checking GitHub…";
  if (!at) return "never synced";
  return `synced ${age(at)} ago`;
}
