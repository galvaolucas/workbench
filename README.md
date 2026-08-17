<img src="src-tauri/icons/128x128.png" width="88" alt="">

# Workbench

A developer's daily driver: the pull requests that want you, and the notes you
keep while dealing with them, in one window that lives in your menu bar.

Local-first, one account per user. Your token stays in your system keychain,
your data stays in a SQLite file on your machine, and there is no server —
nothing about your work leaves your computer.

---

## Why

As a tech lead your attention gets fragmented by tool, not by task. The PR you
opened is in one tab, the three you owe reviews on are in another, the Actions
run you triggered is a third, and the thread where someone `@`-mentioned you is
a fourth. Nothing tells you which of them changed while you were writing code —
and the list of what you actually meant to do today lives somewhere else again.

Workbench is two panes that fix that:

**Today** — one note per day, created for you, with unfinished work carried
forward automatically.

**Desk** — every pull request that involves you, in three lanes, with
notifications when something changes.

## Status

Working today: sign-in, the Desk with background sync and notifications, and
daily notes. Not yet built: the Actions panel, reviewing from inside the app,
and the notes ↔ PR integration. See [Roadmap](#roadmap).

## The Desk

| Lane | What's in it |
| --- | --- |
| **Needs you** | Reviews requested of you, and threads you were mentioned in |
| **Yours in flight** | PRs you opened, with CI status and approvals inline |
| **Watching** | Everything else you're involved in |

Cards show the repo, title, author, `+/−`, CI state, review decision, and a dot
when something changed since you last opened it. Lanes are computed at read
time from relation flags, so the rules can change without a migration or a
re-sync.

**Notifications** fire for exactly four things: a review requested of you, your
PR's checks going red, your PR being approved or having changes requested, and
new comments where you're involved. Everything else is recorded and shown as a
marker on the card, but stays silent. They're coalesced one-per-PR, collapse to
a single summary past three, never fire while you're looking at the app, and
never flood you on first connect.

**Syncing** costs 2 points of GitHub's 5,000/hour budget per pass — and that
cost is flat, because GitHub charges on nodes *requested*, not returned. The
poller follows your attention: 60s focused, 5 min in the background, 15 min
once you've been away a quarter of an hour. Worst case, under 2.5% of budget.

## Today

One note per calendar day, created the first time you look at it. There is no
"new note" button anywhere in this app, by design.

The rule that matters is **carry-forward**: unfinished `- [ ]` items follow you
into today, finished ones stay behind in the day you did them. It looks up the
last day you actually wrote — so Monday picks up Friday's loose ends, not an
empty weekend. Without this, a daily note becomes a graveyard of abandoned
lists within a fortnight.

Notes are stored as **plain text, never as parsed structure**. Todos are counted
by reading the text, so nothing can drift out of sync with what you typed, and
your notes stay readable in any editor if this app disappears.

Notes need no GitHub account. Signing out or switching accounts leaves them
untouched.

## Keyboard

| Keys | Does |
| --- | --- |
| `⌘1` / `⌘2` | Today / Desk |
| `⌘L` | Cycle the current line: text → `- [ ]` → `- [x]` → text |
| `⌘[` / `⌘]` | Previous / next day |

## Running it

Requires **Node 20+** and a **Rust toolchain** (via [rustup](https://rustup.rs)).
If `cargo` isn't found, run `source "$HOME/.cargo/env"` or add `~/.cargo/bin` to
your `PATH`.

```sh
npm install
npm run app          # dev, with hot reload
npm run app:build    # bundled .app
```

### Connecting GitHub

Workbench uses the GitHub **device flow**, which needs a client ID but no client
secret — the reason it can ship in a binary handed to strangers.

1. **GitHub → Settings → Developer settings → OAuth Apps → New OAuth App**
2. Name it anything; homepage URL anything; the callback URL is unused but
   required — `http://localhost` is fine
3. On the app's page, tick **Enable device flow** and save. Without this the
   first connect fails with `device_flow_disabled`
4. Copy the **Client ID** into a `.env` at the repo root:

```sh
cp .env.example .env
# WORKBENCH_GITHUB_CLIENT_ID=Ov23li...
```

Ignore the client secret entirely — this app never sends one.

Scopes requested: `repo read:org notifications read:user`. For GitHub Enterprise
Server, also set `WORKBENCH_GITHUB_HOST=https://ghe.your-co.com`.

`.env` is read in **dev builds only** — a shipped app must not change how it
authenticates based on a stray file in whatever directory it was launched from.
Release builds bake the ID in at compile time, so set it as a real environment
variable:

```sh
WORKBENCH_GITHUB_CLIENT_ID=Ov23li... npm run app:build
```

### macOS notifications

macOS only delivers notifications from a **bundled** app. `npm run app` runs a
raw binary, so notifications there are silent — a platform rule, not a bug. To
test them, build and run the bundle:

```sh
npm run app:build
open "src-tauri/target/release/bundle/macos/Workbench.app"
```

The tray menu's **Send test notification** is deliberately reachable with the
window closed, because that's the state this app lives in.

You'll also be prompted for keychain access on the first run of each new build:
macOS binds keychain rights to an app's code signature, and an unsigned dev
build looks like a different app every time it is compiled. A signed release
build asks once.

## How it works

```
GitHub  ──►  sync engine  ──►  SQLite  ──►  webview  ──►  tray + OS
GraphQL      Rust: poll,       local        React:        notifications
             diff, record      cache +      reads via     fired from Rust
                               event log    invoke
```

Two rules hold the design together.

**The webview never holds the token.** Every GitHub call happens in Rust; every
UI read is a query against local SQLite. A compromised page cannot exfiltrate
credentials, the window opens instantly, and the app works offline.

**`events` is append-only.** Notifications fire from persisted state
transitions, not from what a poll happened to see — so they survive restarts,
cannot double-fire, and cannot miss a change that landed while the app was
closed. The unread markers, and later the digest and weekly stats, all read from
the same table.

The whole Desk is **one GraphQL round trip** — four aliased searches sharing a
fragment that pulls review decisions, comment counts and check rollups
together. Over REST this would be roughly twenty calls.

## Layout

```
src/                    React UI. Never talks to GitHub, never sees a token.
  lib/ipc.ts            The only door: typed wrappers over Tauri commands.
  screens/              Today, Desk, Connect.
src-tauri/src/
  auth/                 AuthProvider trait + the OAuth device flow behind it.
  db.rs                 Schema, migrations, and the append-only event log.
  sync.rs               Fetch, diff, record — and the adaptive poller.
  notes.rs              Daily notes and carry-forward.
  notify.rs             Which transitions are worth interrupting you for.
  keychain.rs           Token storage in the OS keychain.
  github.rs             The GraphQL client.
  tray.rs, commands.rs  Menu bar, and everything the webview may ask for.
```

Auth sits behind a trait on purpose. The device flow needs `repo` scope, which
is all-or-nothing and which security-conscious orgs refuse; the answer is a
GitHub App (per-org install, fine-grained permissions, device flow works there
too). Swapping providers after you have users means re-onboarding all of them,
so the seam exists from day one.

## Roadmap

- [x] Sign-in, keychain, tray, notifications
- [x] The Desk — one GraphQL query, three lanes, diff engine
- [x] Background sync, adaptive cadence, notification rules
- [x] Daily notes with carry-forward
- [ ] Actions panel — job list and failing-step logs inline
- [ ] Review from inside the app — diff, comment, approve, merge
- [ ] Push a PR into today's note; its checkbox ticks itself when the PR merges
- [ ] Clickable checkboxes, `.md` export, search UI
- [ ] Review load, aging report, morning digest
- [ ] Multi-account, auto-update, code signing

## Built with

[Tauri 2](https://tauri.app) · React · TypeScript · SQLite · GitHub GraphQL API
