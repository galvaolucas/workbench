import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight } from "@/components/icons";
import { noteOpen, noteSave, type Note } from "@/lib/ipc";

const SAVE_DEBOUNCE_MS = 600;

export default function Today() {
  const [note, setNote] = useState<Note | null>(null);
  const [body, setBody] = useState("");
  const [saved, setSaved] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const saveTimer = useRef<number | undefined>(undefined);
  const dayRef = useRef<string | null>(null);

  const go = useCallback(async (day?: string) => {
    try {
      const next = await noteOpen(day);
      setNote(next);
      setBody(next.body);
      dayRef.current = next.day;
      setSaved(true);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void go();
  }, [go]);

  // If the app is left open overnight, the day has to turn over on its own.
  useEffect(() => {
    const id = window.setInterval(() => {
      if (note?.isToday && dayRef.current !== localDay()) void go();
    }, 30_000);
    return () => window.clearInterval(id);
  }, [note?.isToday, go]);

  // Autosave. No save button, no dirty-state prompt: the note is always saved,
  // you just occasionally see it happen.
  const edit = useCallback((next: string) => {
    setBody(next);
    setSaved(false);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      const day = dayRef.current;
      if (!day) return;
      noteSave(day, next)
        .then((updated) => {
          setNote(updated);
          setSaved(true);
        })
        .catch((e) => setError(String(e)));
    }, SAVE_DEBOUNCE_MS);
  }, []);

  // Flush on unmount so switching tabs mid-sentence never loses a keystroke.
  useEffect(
    () => () => {
      window.clearTimeout(saveTimer.current);
      const day = dayRef.current;
      if (day && areaRef.current) void noteSave(day, areaRef.current.value);
    },
    [],
  );

  function onKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (!e.metaKey) return;
    if (e.key === "l") {
      e.preventDefault();
      toggleCurrentLine();
    } else if (e.key === "[") {
      e.preventDefault();
      if (note?.previousDay) void go(note.previousDay);
    } else if (e.key === "]") {
      e.preventDefault();
      if (note?.nextDay) void go(note.nextDay);
    }
  }

  /** ⌘L turns the line under the cursor into a todo, ticks it, or unticks it. */
  function toggleCurrentLine() {
    const area = areaRef.current;
    if (!area) return;

    const value = area.value;
    const caret = area.selectionStart;
    const start = value.lastIndexOf("\n", caret - 1) + 1;
    const endIndex = value.indexOf("\n", caret);
    const end = endIndex === -1 ? value.length : endIndex;
    const line = value.slice(start, end);

    const match = /^(\s*)([-*] )?(\[( |x|X)\] )?(.*)$/.exec(line);
    if (!match) return;
    const [, indent, , checkbox, mark, rest] = match;

    let replacement: string;
    if (!checkbox) replacement = `${indent}- [ ] ${rest}`;
    else if (mark === " ") replacement = `${indent}- [x] ${rest}`;
    else replacement = `${indent}${rest}`;

    const next = value.slice(0, start) + replacement + value.slice(end);
    const shift = replacement.length - line.length;
    edit(next);
    requestAnimationFrame(() => {
      area.selectionStart = area.selectionEnd = Math.max(start, caret + shift);
    });
  }

  return (
    <>
      <header className="topbar" data-tauri-drag-region>
        <h1 className="page-title">{note ? describeDay(note.day) : ""}</h1>
        <div className="topbar-side">
          {note && !note.isToday && (
            <button className="btn" onClick={() => void go()}>
              Back to today
            </button>
          )}
          <button
            className="icon-btn"
            onClick={() => note?.previousDay && void go(note.previousDay)}
            disabled={!note?.previousDay}
            title="Previous day (⌘[)"
          >
            <ChevronLeft />
          </button>
          <button
            className="icon-btn"
            onClick={() => note?.nextDay && void go(note.nextDay)}
            disabled={!note?.nextDay}
            title="Next day (⌘])"
          >
            <ChevronRight />
          </button>
        </div>
      </header>

      <div className="content">
        {error && <p className="error">{error}</p>}

        <div className="card note-card">
          <textarea
            ref={areaRef}
            className="note"
            value={body}
            onChange={(e) => edit(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder={"What needs doing today?\n\n- [ ] start here  (⌘L toggles a line)"}
            spellCheck={false}
          />
          <footer className="note-foot">
            <span className="tallies">
              {note && (
                <>
                  <span className="chip ok">{note.done} done</span>
                  <span className="chip warn">{note.open} open</span>
                </>
              )}
            </span>
            <span className="saved">{saved ? "saved" : "saving…"}</span>
          </footer>
        </div>
      </div>
    </>
  );
}

function localDay(date = new Date()) {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function describeDay(day: string) {
  const [y, m, d] = day.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  const today = localDay();
  if (day === today) return "Today";

  const yesterday = new Date();
  yesterday.setDate(yesterday.getDate() - 1);
  if (day === localDay(yesterday)) return "Yesterday";

  return date.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}
