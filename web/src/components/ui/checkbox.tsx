import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { Check } from "lucide-react";
import type { ComponentPropsWithoutRef, ElementRef } from "react";
import { forwardRef } from "react";
import { cn } from "../../utils/cn";

// shadcn/ui-compatible Radix checkbox wrapper backed by project CSS tokens.
const Checkbox = forwardRef<ElementRef<typeof CheckboxPrimitive.Root>, ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>>(
  ({ className, ...props }, ref) => (
    <CheckboxPrimitive.Root ref={ref} className={cn("ui-checkbox", className)} {...props}>
      <CheckboxPrimitive.Indicator className="ui-checkbox-indicator">
        <Check size={12} strokeWidth={2.5} />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  ),
);
Checkbox.displayName = CheckboxPrimitive.Root.displayName;

export { Checkbox };
