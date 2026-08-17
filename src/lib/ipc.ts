/**
 * The only door between the webview and the outside world.
 *
 * Nothing in `src/` may talk to GitHub directly: the token lives in the OS
 * keychain and is only ever read inside Rust. Every fetch is a command here,
 * every read is a query against the local database.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Account = {
  id: number;
  login: string;
  name: string | null;
  avatarUrl: string | null;
  host: string;
  provider: string;
  connectedAt: number;
};

export type DeviceStart = {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
};

export type AppInfo = {
  version: string;
  dbPath: string;
  schemaVersion: number;
  apiBase: string;
  webBase: string;
  provider: string;
  clientIdConfigured: boolean;
};

export const authStatus = () => invoke<Account | null>("auth_status");

/**
 * Starts the device flow and returns the code to show the user. Polling
 * continues in the background — listen for `auth:completed` / `auth:failed`
 * rather than awaiting this call.
 */
export const authBegin = () => invoke<DeviceStart>("auth_begin");

export const authCancel = () => invoke<void>("auth_cancel");

export const authLogout = () => invoke<void>("auth_logout");

export const appInfo = () => invoke<AppInfo>("app_info");

export const sendTestNotification = () => invoke<void>("send_test_notification");

export const hideWindow = () => invoke<void>("hide_window");

export const openExternal = (url: string) => invoke<void>("open_external", { url });

export const onAuthCompleted = (fn: (account: Account) => void): Promise<UnlistenFn> =>
  listen<Account>("auth:completed", (e) => fn(e.payload));

export const onAuthFailed = (fn: (message: string) => void): Promise<UnlistenFn> =>
  listen<string>("auth:failed", (e) => fn(e.payload));

export type PullRequest = {
  id: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  author: string | null;
  authorAvatar: string | null;
  isDraft: boolean;
  additions: number;
  deletions: number;
  changedFiles: number;
  commentCount: number;
  reviewDecision: "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null;
  checksState: "SUCCESS" | "FAILURE" | "PENDING" | "ERROR" | "EXPECTED" | null;
  isAuthor: boolean;
  isReviewer: boolean;
  isMentioned: boolean;
  updatedAt: number;
  lane: "needs_you" | "yours" | "watching";
  /** Event kinds since you last opened it — the "what's new" markers. */
  unread: string[];
};

export type Desk = {
  needsYou: PullRequest[];
  yours: PullRequest[];
  watching: PullRequest[];
  lastSyncedAt: number | null;
  /** Orgs this token can reach. Anything missing is invisible to search. */
  visibleOrgs: string[];
  orgAccessUrl: string | null;
};

export type SyncOutcome = {
  pullRequests: number;
  events: number;
  retired: number;
  cost: number;
  remaining: number;
  syncedAt: number;
};

/** Local read. Never hits the network — that's what syncNow is for. */
export const desk = () => invoke<Desk>("desk");

export const syncNow = () => invoke<SyncOutcome>("sync_now");

export const openPullRequest = (id: string, url: string) =>
  invoke<void>("open_pull_request", { id, url });

export const onDeskUpdated = (fn: (outcome: SyncOutcome) => void): Promise<UnlistenFn> =>
  listen<SyncOutcome>("desk:updated", (e) => fn(e.payload));

export type Note = {
  day: string;
  body: string;
  updatedAt: number;
  isToday: boolean;
  previousDay: string | null;
  nextDay: string | null;
  done: number;
  open: number;
};

/** Omit `day` for today. Days are created lazily — there is no "new note". */
export const noteOpen = (day?: string) => invoke<Note>("note_open", { day: day ?? null });

export const noteSave = (day: string, body: string) =>
  invoke<Note>("note_save", { day, body });

export const noteSearch = (query: string) =>
  invoke<{ day: string; body: string; updatedAt: number }[]>("note_search", { query });

export type Settings = {
  /** Where notes are written. null means the app's own database. */
  notesDir: string | null;
  dbPath: string;
  version: string;
  schemaVersion: number;
  apiBase: string;
  account: Account | null;
};

export const settingsRead = () => invoke<Settings>("settings_read");

/** Returns how many notes were copied into the folder. */
export const settingsSetNotesDir = (path: string) =>
  invoke<number>("settings_set_notes_dir", { path });

export const settingsClearNotesDir = () => invoke<void>("settings_clear_notes_dir");

export const settingsRevealNotes = () => invoke<void>("settings_reveal_notes");
