import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";

type TooltipState = {
  anchor: HTMLElement;
  text: string;
};

type TooltipPosition = {
  left: number;
  top: number;
  arrowLeft: number;
  placement: "top" | "bottom";
  maxWidth: number;
};

const VIEWPORT_MARGIN = 12;
const TOOLTIP_GAP = 9;
const ARROW_MIN_OFFSET = 14;
const HOVER_SHOW_DELAY_MS = 1000;

function tooltipText(element: HTMLElement | null) {
  const text = element?.getAttribute("data-tooltip")?.trim();
  return text || null;
}

function tooltipAnchor(target: EventTarget | null) {
  if (!(target instanceof Element)) return null;
  const element = target.closest("[data-tooltip]");
  return element instanceof HTMLElement ? element : null;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function positionTooltip(anchor: HTMLElement, tooltip: HTMLElement): TooltipPosition | null {
  if (!anchor.isConnected) return null;
  const rect = anchor.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return null;

  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const maxWidth = Math.max(160, Math.min(360, viewportWidth - VIEWPORT_MARGIN * 2));
  tooltip.style.maxWidth = `${maxWidth}px`;
  const tooltipRect = tooltip.getBoundingClientRect();
  const width = Math.min(tooltipRect.width, maxWidth);
  const height = tooltipRect.height;
  const anchorCenter = rect.left + rect.width / 2;
  const left = clamp(
    anchorCenter - width / 2,
    VIEWPORT_MARGIN,
    Math.max(VIEWPORT_MARGIN, viewportWidth - VIEWPORT_MARGIN - width),
  );

  const bottomTop = rect.bottom + TOOLTIP_GAP;
  const topTop = rect.top - TOOLTIP_GAP - height;
  const hasBottomRoom = bottomTop + height <= viewportHeight - VIEWPORT_MARGIN;
  const hasTopRoom = topTop >= VIEWPORT_MARGIN;
  const placement = hasBottomRoom || !hasTopRoom ? "bottom" : "top";
  const rawTop = placement === "bottom" ? bottomTop : topTop;
  const top = clamp(rawTop, VIEWPORT_MARGIN, Math.max(VIEWPORT_MARGIN, viewportHeight - VIEWPORT_MARGIN - height));
  const arrowLeft = clamp(anchorCenter - left, ARROW_MIN_OFFSET, Math.max(ARROW_MIN_OFFSET, width - ARROW_MIN_OFFSET));

  return { left, top, arrowLeft, placement, maxWidth };
}

export function TooltipLayer({ theme }: { theme: "dark" | "light" }) {
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);
  const [position, setPosition] = useState<TooltipPosition | null>(null);
  const tooltipRef = useRef<HTMLDivElement | null>(null);
  const hoverTimerRef = useRef<number | null>(null);
  const pendingAnchorRef = useRef<HTMLElement | null>(null);
  const hoveredAnchorRef = useRef<HTMLElement | null>(null);

  const clearHoverTimer = useCallback(() => {
    if (hoverTimerRef.current !== null) {
      window.clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
    pendingAnchorRef.current = null;
  }, []);

  const hide = useCallback(() => {
    clearHoverTimer();
    hoveredAnchorRef.current = null;
    setTooltip(null);
    setPosition(null);
  }, [clearHoverTimer]);

  const show = useCallback((anchor: HTMLElement | null) => {
    const text = tooltipText(anchor);
    if (!anchor || !text) {
      hide();
      return;
    }
    clearHoverTimer();
    setTooltip({ anchor, text });
    setPosition(null);
  }, [clearHoverTimer, hide]);

  const scheduleShow = useCallback((anchor: HTMLElement | null) => {
    const text = tooltipText(anchor);
    if (!anchor || !text) {
      hide();
      return;
    }
    clearHoverTimer();
    pendingAnchorRef.current = anchor;
    hoverTimerRef.current = window.setTimeout(() => {
      hoverTimerRef.current = null;
      const currentText = tooltipText(anchor);
      if (
        pendingAnchorRef.current !== anchor ||
        hoveredAnchorRef.current !== anchor ||
        !anchor.isConnected ||
        !currentText
      ) {
        pendingAnchorRef.current = null;
        return;
      }
      pendingAnchorRef.current = null;
      setTooltip({ anchor, text: currentText });
      setPosition(null);
    }, HOVER_SHOW_DELAY_MS);
  }, [clearHoverTimer, hide]);

  const updatePosition = useCallback(() => {
    if (!tooltip || !tooltipRef.current) return;
    const next = positionTooltip(tooltip.anchor, tooltipRef.current);
    if (!next) {
      hide();
      return;
    }
    setPosition(next);
  }, [hide, tooltip]);

  useLayoutEffect(() => {
    updatePosition();
  }, [updatePosition]);

  useEffect(() => {
    const handleMouseOver = (event: MouseEvent) => {
      const anchor = tooltipAnchor(event.target);
      if (!anchor) return;
      const related = event.relatedTarget;
      if (related instanceof Node && anchor.contains(related)) return;
      hoveredAnchorRef.current = anchor;
      scheduleShow(anchor);
    };

    const handleMouseOut = (event: MouseEvent) => {
      const anchor = tooltipAnchor(event.target);
      const related = event.relatedTarget;
      if (anchor && related instanceof Node && anchor.contains(related)) return;
      if (anchor && hoveredAnchorRef.current === anchor) {
        hoveredAnchorRef.current = null;
      }
      if (anchor && pendingAnchorRef.current === anchor) {
        clearHoverTimer();
      }
      if (!tooltip) return;
      if (related instanceof Node && tooltip.anchor.contains(related)) return;
      hide();
    };

    const handleFocusIn = (event: FocusEvent) => {
      show(tooltipAnchor(event.target));
    };

    const handleFocusOut = (event: FocusEvent) => {
      if (!tooltip) return;
      const related = event.relatedTarget;
      if (related instanceof Node && tooltip.anchor.contains(related)) return;
      hide();
    };

    document.addEventListener("mouseover", handleMouseOver);
    document.addEventListener("mouseout", handleMouseOut);
    document.addEventListener("focusin", handleFocusIn);
    document.addEventListener("focusout", handleFocusOut);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);

    return () => {
      document.removeEventListener("mouseover", handleMouseOver);
      document.removeEventListener("mouseout", handleMouseOut);
      document.removeEventListener("focusin", handleFocusIn);
      document.removeEventListener("focusout", handleFocusOut);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
      clearHoverTimer();
    };
  }, [clearHoverTimer, hide, scheduleShow, show, tooltip, updatePosition]);

  if (!tooltip) return null;

  const style: CSSProperties = {
    left: position?.left ?? VIEWPORT_MARGIN,
    top: position?.top ?? VIEWPORT_MARGIN,
    maxWidth: position?.maxWidth,
    visibility: position ? "visible" : "hidden",
  };
  const arrowStyle: CSSProperties = {
    left: position?.arrowLeft ?? ARROW_MIN_OFFSET,
  };

  return createPortal(
    <div
      className={`app-tooltip ${theme}`}
      data-placement={position?.placement ?? "bottom"}
      ref={tooltipRef}
      role="tooltip"
      style={style}
    >
      <span className="app-tooltip-arrow" style={arrowStyle} />
      {tooltip.text}
    </div>,
    document.body,
  );
}
