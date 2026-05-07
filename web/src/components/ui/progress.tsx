import type { HTMLAttributes } from "react";
import { cn } from "../../utils/cn";

type ProgressProps = HTMLAttributes<HTMLDivElement> & {
  value?: number;
};

export function Progress({ className, value = 0, ...props }: ProgressProps) {
  return (
    <div className={cn("ui-progress", className)} {...props}>
      <div className="ui-progress-indicator" style={{ width: `${Math.min(100, Math.max(0, value))}%` }} />
    </div>
  );
}
