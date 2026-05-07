import type { HTMLAttributes } from "react";
import { cn } from "../../utils/cn";

export function Badge({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("ui-badge", className)} {...props} />;
}
