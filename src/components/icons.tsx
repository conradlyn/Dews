/**
 * Small line icons drawn inline so they inherit `currentColor` and stay crisp
 * at 13px. Keeping them local avoids an icon-font dependency.
 */
export type IconName =
  | "all"
  | "unread"
  | "read"
  | "check-all"
  | "add-feed"
  | "sync"
  | "account"
  | "settings";

interface Props {
  name: IconName;
  size?: number;
  className?: string;
}

export default function Icon({ name, size = 13, className }: Props) {
  const base = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
    focusable: false,
    className,
  };

  switch (name) {
    case "all":
      return (
        <svg {...base}>
          <path d="M8 6h12M8 12h12M8 18h12" />
          <circle cx="4" cy="6" r="1.1" fill="currentColor" stroke="none" />
          <circle cx="4" cy="12" r="1.1" fill="currentColor" stroke="none" />
          <circle cx="4" cy="18" r="1.1" fill="currentColor" stroke="none" />
        </svg>
      );
    case "unread":
      return (
        <svg {...base}>
          <circle cx="12" cy="12" r="8" />
          <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none" />
        </svg>
      );
    case "read":
      return (
        <svg {...base}>
          <circle cx="12" cy="12" r="8" />
          <path d="M8.4 12.4l2.6 2.4 4.7-5" />
        </svg>
      );
    case "check-all":
      return (
        <svg {...base}>
          <path d="M2.5 12.6l3.4 3.4 7-7.6" />
          <path d="M9.6 15.6l1.5 1.5 8.4-9" />
        </svg>
      );
    case "add-feed":
      return (
        <svg {...base}>
          <path d="M4 5h11M4 11h11M4 17h6" />
          <circle cx="18" cy="17" r="3.2" />
          <path d="M18 15.2v3.6M16.2 17h3.6" />
        </svg>
      );
    case "sync":
      return (
        <svg {...base}>
          <path d="M20 11a8 8 0 0 0-14.3-4.5L3 9" />
          <path d="M3 4v5h5" />
          <path d="M4 13a8 8 0 0 0 14.3 4.5L21 15" />
          <path d="M21 20v-5h-5" />
        </svg>
      );
    case "account":
      return (
        <svg {...base}>
          <circle cx="12" cy="8" r="3.6" />
          <path d="M4.9 20c0-3.5 3.2-6.1 7.1-6.1s7.1 2.6 7.1 6.1" />
        </svg>
      );
    case "settings":
      return (
        <svg {...base}>
          <path d="M5 21v-6M5 11V3M12 21v-8M12 9V3M19 21v-4M19 13V3" />
          <path d="M2 15h6M9 9h6M16 17h6" />
        </svg>
      );
    default:
      return null;
  }
}
