import { describe, expect, it } from "vitest";
import { createScrollController } from "../src/lib/scroll";

function fakeEl(opts: {
  scrollHeight: number;
  clientHeight: number;
  scrollTop: number;
}): HTMLElement {
  const el = {
    scrollHeight: opts.scrollHeight,
    clientHeight: opts.clientHeight,
    scrollTop: opts.scrollTop,
  };
  return el as unknown as HTMLElement;
}

describe("scroll follow state machine", () => {
  it("starts following-tail and follows content growth", () => {
    const c = createScrollController(48, 250);
    expect(c.state).toBe("following-tail");
    const el = fakeEl({ scrollHeight: 1000, clientHeight: 400, scrollTop: 552 });
    // near bottom: 1000-552-400=48
    expect(c.isNearBottom(el)).toBe(true);
    expect(c.maybeFollow(el)).toBe(true);
    expect(el.scrollTop).toBe(1000);
  });

  it("detaches only on user intent + leaving bottom", () => {
    const c = createScrollController(48, 250);
    const el = fakeEl({ scrollHeight: 1000, clientHeight: 400, scrollTop: 100 });
    // layout scroll without intent — stay following
    c.onScroll(el);
    expect(c.state).toBe("following-tail");

    c.markUserIntent();
    c.onScroll(el);
    expect(c.state).toBe("detached-by-user");
    expect(c.maybeFollow(el)).toBe(false);
    expect(el.scrollTop).toBe(100);
  });

  it("re-follows when user scrolls back to bottom with intent", () => {
    const c = createScrollController(48, 250);
    c.markUserIntent();
    const away = fakeEl({ scrollHeight: 1000, clientHeight: 400, scrollTop: 0 });
    c.onScroll(away);
    expect(c.state).toBe("detached-by-user");

    c.markUserIntent();
    const bottom = fakeEl({
      scrollHeight: 1000,
      clientHeight: 400,
      scrollTop: 560,
    });
    c.onScroll(bottom);
    expect(c.state).toBe("following-tail");
    expect(c.unreadCount).toBe(0);
  });

  it("jumpToLatest forces following-tail and clears unread", () => {
    const c = createScrollController();
    c.state = "detached-by-user";
    c.unreadCount = 5;
    const el = fakeEl({ scrollHeight: 800, clientHeight: 300, scrollTop: 0 });
    c.jumpToLatest(el);
    expect(c.state).toBe("following-tail");
    expect(el.scrollTop).toBe(800);
    expect(c.unreadCount).toBe(0);
  });

  it("increments unread only while detached", () => {
    const c = createScrollController();
    c.notifyContentGrowth({ newItemCount: 2, lastSequence: 10 });
    expect(c.unreadCount).toBe(0);

    c.state = "detached-by-user";
    c.notifyContentGrowth({ newItemCount: 3, lastSequence: 13 });
    expect(c.unreadCount).toBe(3);
    c.notifyContentGrowth({ newItemCount: 1 });
    expect(c.unreadCount).toBe(4);
  });

  it("does not auto-follow when detached after growth", () => {
    const c = createScrollController();
    c.markUserIntent();
    const el = fakeEl({ scrollHeight: 1000, clientHeight: 400, scrollTop: 50 });
    c.onScroll(el);
    expect(c.state).toBe("detached-by-user");
    const topBefore = el.scrollTop;
    el.scrollHeight = 1500; // content grew
    expect(c.maybeFollow(el)).toBe(false);
    expect(el.scrollTop).toBe(topBefore);
  });

  it("snapshot/restore preserves detach and unread", () => {
    const c = createScrollController();
    c.state = "detached-by-user";
    c.unreadCount = 2;
    c.lastSeenSequence = 42;
    c.anchor = { anchorItemId: "seg:1", intraItemOffsetPx: 12 };
    const snap = c.snapshot();
    const c2 = createScrollController();
    c2.restore(snap);
    expect(c2.state).toBe("detached-by-user");
    expect(c2.unreadCount).toBe(2);
    expect(c2.lastSeenSequence).toBe(42);
    expect(c2.anchor.anchorItemId).toBe("seg:1");
  });
});
