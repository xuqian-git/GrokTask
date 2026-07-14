/**
 * Bottom-follow scroll state machine (conversation-stream.md §7).
 * following-tail <-> detached-by-user
 */

export type ScrollFollowState = "following-tail" | "detached-by-user";

export const BOTTOM_THRESHOLD_PX = 48;
export const USER_INTENT_WINDOW_MS = 250;

export interface ScrollAnchor {
  anchorItemId?: string;
  intraItemOffsetPx?: number;
  lastSeenSequence?: number;
}

export interface ScrollController {
  state: ScrollFollowState;
  lastUserIntentAt: number;
  /** Count of new items / growth while detached. */
  unreadCount: number;
  lastSeenSequence: number;
  anchor: ScrollAnchor;
  /** Call on wheel / touchmove / scrollbar drag start. */
  markUserIntent(): void;
  /** Whether the element is within the bottom threshold. */
  isNearBottom(el: HTMLElement): boolean;
  /**
   * On scroll event: if user intent was recent and left bottom, detach.
   * If user scrolled back to bottom with intent, re-follow.
   */
  onScroll(el: HTMLElement): void;
  /**
   * Programmatic content growth: only scroll if following-tail.
   * Returns true if scrollTop was updated.
   */
  maybeFollow(el: HTMLElement): boolean;
  /**
   * Notify that new timeline content arrived while possibly detached.
   * Increments unread when detached; no-op when following.
   */
  notifyContentGrowth(opts?: {
    newItemCount?: number;
    lastSequence?: number;
  }): void;
  /** Explicit jump-to-latest. Clears unread. */
  jumpToLatest(el: HTMLElement): void;
  /** Snapshot for surface restore (popover hide/show). */
  snapshot(): {
    state: ScrollFollowState;
    unreadCount: number;
    lastSeenSequence: number;
    anchor: ScrollAnchor;
  };
  restore(snap: {
    state: ScrollFollowState;
    unreadCount: number;
    lastSeenSequence: number;
    anchor?: ScrollAnchor;
  }): void;
}

export function createScrollController(
  threshold = BOTTOM_THRESHOLD_PX,
  intentWindowMs = USER_INTENT_WINDOW_MS,
): ScrollController {
  const ctrl: ScrollController = {
    state: "following-tail",
    lastUserIntentAt: 0,
    unreadCount: 0,
    lastSeenSequence: 0,
    anchor: {},
    markUserIntent() {
      ctrl.lastUserIntentAt = Date.now();
    },
    isNearBottom(el: HTMLElement) {
      const remaining = el.scrollHeight - el.scrollTop - el.clientHeight;
      return remaining <= threshold;
    },
    onScroll(el: HTMLElement) {
      const now = Date.now();
      const recentIntent = now - ctrl.lastUserIntentAt <= intentWindowMs;
      if (!recentIntent) {
        // Programmatic / layout scroll — do not change follow state
        return;
      }
      if (ctrl.isNearBottom(el)) {
        ctrl.state = "following-tail";
        ctrl.unreadCount = 0;
      } else {
        ctrl.state = "detached-by-user";
      }
    },
    maybeFollow(el: HTMLElement) {
      if (ctrl.state !== "following-tail") return false;
      el.scrollTop = el.scrollHeight;
      return true;
    },
    notifyContentGrowth(opts = {}) {
      if (typeof opts.lastSequence === "number") {
        ctrl.lastSeenSequence = Math.max(
          ctrl.lastSeenSequence,
          opts.lastSequence,
        );
      }
      if (ctrl.state === "detached-by-user") {
        const n = opts.newItemCount ?? 1;
        if (n > 0) ctrl.unreadCount += n;
      } else {
        ctrl.unreadCount = 0;
      }
    },
    jumpToLatest(el: HTMLElement) {
      ctrl.state = "following-tail";
      ctrl.unreadCount = 0;
      el.scrollTop = el.scrollHeight;
    },
    snapshot() {
      return {
        state: ctrl.state,
        unreadCount: ctrl.unreadCount,
        lastSeenSequence: ctrl.lastSeenSequence,
        anchor: { ...ctrl.anchor },
      };
    },
    restore(snap) {
      ctrl.state = snap.state;
      ctrl.unreadCount = snap.unreadCount;
      ctrl.lastSeenSequence = snap.lastSeenSequence;
      ctrl.anchor = { ...(snap.anchor ?? {}) };
    },
  };
  return ctrl;
}

/**
 * Attach DOM listeners that mark user scroll intent.
 * Wheel, touchmove, and pointerdown on scrollbar-ish regions.
 * Returns a cleanup function.
 */
export function attachScrollIntentListeners(
  el: HTMLElement,
  ctrl: ScrollController,
): () => void {
  const onWheel = () => ctrl.markUserIntent();
  const onTouchMove = () => ctrl.markUserIntent();
  // Pointer down near the right edge approximates scrollbar drag
  const onPointerDown = (e: PointerEvent) => {
    const rect = el.getBoundingClientRect();
    const fromRight = rect.right - e.clientX;
    if (fromRight <= 16) ctrl.markUserIntent();
  };

  el.addEventListener("wheel", onWheel, { passive: true });
  el.addEventListener("touchmove", onTouchMove, { passive: true });
  el.addEventListener("pointerdown", onPointerDown);

  return () => {
    el.removeEventListener("wheel", onWheel);
    el.removeEventListener("touchmove", onTouchMove);
    el.removeEventListener("pointerdown", onPointerDown);
  };
}
