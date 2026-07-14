/**
 * Bottom-follow scroll state machine (conversation-stream.md §7).
 * following-tail <-> detached-by-user
 */

export type ScrollFollowState = "following-tail" | "detached-by-user";

export const BOTTOM_THRESHOLD_PX = 48;
export const USER_INTENT_WINDOW_MS = 250;

export interface ScrollController {
  state: ScrollFollowState;
  lastUserIntentAt: number;
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
  /** Explicit jump-to-latest. */
  jumpToLatest(el: HTMLElement): void;
}

export function createScrollController(
  threshold = BOTTOM_THRESHOLD_PX,
  intentWindowMs = USER_INTENT_WINDOW_MS,
): ScrollController {
  const ctrl: ScrollController = {
    state: "following-tail",
    lastUserIntentAt: 0,
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
      } else {
        ctrl.state = "detached-by-user";
      }
    },
    maybeFollow(el: HTMLElement) {
      if (ctrl.state !== "following-tail") return false;
      el.scrollTop = el.scrollHeight;
      return true;
    },
    jumpToLatest(el: HTMLElement) {
      ctrl.state = "following-tail";
      el.scrollTop = el.scrollHeight;
    },
  };
  return ctrl;
}
