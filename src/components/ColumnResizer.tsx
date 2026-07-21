import { useEffect, useRef, type KeyboardEvent, type PointerEvent } from "react";

interface Props {
  value: number;
  defaultValue: number;
  min: number;
  getMax: () => number;
  onChange: (value: number) => void;
  label: string;
  testId: string;
}

function clamp(value: number, min: number, max: number): number {
  return Math.round(Math.min(Math.max(max, min), Math.max(min, value)));
}

export function ColumnResizer({
  value,
  defaultValue,
  min,
  getMax,
  onChange,
  label,
  testId,
}: Props) {
  const cleanupRef = useRef<(() => void) | null>(null);

  const stopDragging = () => {
    cleanupRef.current?.();
    cleanupRef.current = null;
  };

  useEffect(() => () => cleanupRef.current?.(), []);

  const startDragging = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    stopDragging();
    const startX = event.clientX;
    const startValue = value;
    document.body.classList.add("column-resizing");

    const move = (moveEvent: globalThis.PointerEvent) => {
      onChange(clamp(startValue + moveEvent.clientX - startX, min, getMax()));
    };
    const stop = () => stopDragging();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
    window.addEventListener("pointercancel", stop, { once: true });
    cleanupRef.current = () => {
      document.body.classList.remove("column-resizing");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = value - (event.shiftKey ? 40 : 10);
    if (event.key === "ArrowRight") next = value + (event.shiftKey ? 40 : 10);
    if (event.key === "Home") next = defaultValue;
    if (next == null) return;
    event.preventDefault();
    onChange(clamp(next, min, getMax()));
  };

  return (
    <div
      className="column-resizer"
      data-testid={testId}
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={Math.max(min, getMax())}
      aria-valuenow={value}
      tabIndex={0}
      title={`${label}（ドラッグ、矢印キー、ダブルクリックでリセット）`}
      onPointerDown={startDragging}
      onKeyDown={handleKeyDown}
      onDoubleClick={() => onChange(clamp(defaultValue, min, getMax()))}
    />
  );
}
