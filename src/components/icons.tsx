/**
 * Inline SVG only — the artifact CSP blocks external requests, and an icon
 * font would be a second typeface to load for a dozen glyphs.
 *
 * All icons inherit `currentColor` and size from the CSS box, so a button's
 * colour states apply to its icon without extra rules.
 */
type Props = { size?: number; className?: string };

const base = (size: number) => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
});

/** The mark: sun over a horizon. Same shape as the app icon. */
export function Sunrise({ size = 20, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M3.5 18h17" />
      <path d="M7 18a5 5 0 0 1 10 0" />
      <path d="M12 3.5v2.4" />
      <path d="M4.9 7.4 6.6 9.1" />
      <path d="M19.1 7.4 17.4 9.1" />
    </svg>
  );
}

/** Today: a page with a checked line. */
export function Notes({ size = 20, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <rect x="4.5" y="3.5" width="15" height="17" rx="3.5" />
      <path d="m8 9 1.6 1.6L13 7.4" />
      <path d="M8 14.5h8" />
      <path d="M8 17.5h5" />
    </svg>
  );
}

/** Desk: stacked lanes. */
export function Lanes({ size = 20, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <rect x="3.5" y="4.5" width="5.5" height="15" rx="2" />
      <rect x="11.5" y="4.5" width="9" height="7" rx="2" />
      <rect x="11.5" y="14.5" width="9" height="5" rx="2" />
    </svg>
  );
}

export function Refresh({ size = 16, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M20 12a8 8 0 1 1-2.6-5.9" />
      <path d="M20 4v4.5h-4.5" />
    </svg>
  );
}

export function ChevronLeft({ size = 16, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M14.5 5 8 12l6.5 7" />
    </svg>
  );
}

export function ChevronRight({ size = 16, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M9.5 5 16 12l-6.5 7" />
    </svg>
  );
}

export function SignOut({ size = 20, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M15 4.5h2.5a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H15" />
      <path d="M10 8.5 13.5 12 10 15.5" />
      <path d="M13.5 12h-9" />
    </svg>
  );
}

export function Gear({ size = 20, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <circle cx="12" cy="12" r="3.2" />
      <path d="M12 3.5v2M12 18.5v2M3.5 12h2M18.5 12h2M6 6l1.4 1.4M16.6 16.6 18 18M18 6l-1.4 1.4M7.4 16.6 6 18" />
    </svg>
  );
}

export function Folder({ size = 16, className }: Props) {
  return (
    <svg {...base(size)} className={className} aria-hidden="true">
      <path d="M3.5 7a2 2 0 0 1 2-2h3.2l2 2.5h7.8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
    </svg>
  );
}
