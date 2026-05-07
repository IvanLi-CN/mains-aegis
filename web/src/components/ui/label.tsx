import * as LabelPrimitive from "@radix-ui/react-label";
import { forwardRef } from "react";
import type { ComponentPropsWithoutRef, ElementRef } from "react";
import { cn } from "../../utils/cn";

const Label = forwardRef<ElementRef<typeof LabelPrimitive.Root>, ComponentPropsWithoutRef<typeof LabelPrimitive.Root>>(
  ({ className, ...props }, ref) => <LabelPrimitive.Root ref={ref} className={cn("ui-label", className)} {...props} />,
);
Label.displayName = LabelPrimitive.Root.displayName;

export { Label };
