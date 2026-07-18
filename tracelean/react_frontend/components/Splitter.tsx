import { useCallback } from "react";

interface SplitterProps {
  /** CSS variable the adjacent panel's width reads (e.g. "--file-tree-width"). */
  cssVar: string;
  /** Which side of the splitter the panel sits on. */
  side: "left" | "right";
  defaultWidth: number;
  min?: number;
  max?: number;
}

/** Draggable divider: drag to resize the panel bound to `cssVar`. */
export function Splitter({ cssVar, side, defaultWidth, min = 160, max = 700 }: SplitterProps) {
  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      const root = document.documentElement;
      const current = parseInt(getComputedStyle(root).getPropertyValue(cssVar), 10);
      const startWidth = Number.isFinite(current) && current > 0 ? current : defaultWidth;
      const startX = e.clientX;
      const move = (ev: PointerEvent) => {
        const dx = ev.clientX - startX;
        const w = side === "left" ? startWidth + dx : startWidth - dx;
        root.style.setProperty(cssVar, `${Math.min(max, Math.max(min, w))}px`);
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        document.body.style.cursor = "";
      };
      document.body.style.cursor = "col-resize";
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [cssVar, side, defaultWidth, min, max]
  );

  return <div className="splitter" onPointerDown={onPointerDown} />;
}
