import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ComponentPropsWithoutRef, ElementRef, HTMLAttributes } from "react";
import { forwardRef } from "react";
import { cn } from "../../utils/cn";
import { Button } from "./button";

const Sheet = DialogPrimitive.Root;
const SheetTrigger = DialogPrimitive.Trigger;
const SheetClose = DialogPrimitive.Close;
const SheetPortal = DialogPrimitive.Portal;

const SheetOverlay = forwardRef<ElementRef<typeof DialogPrimitive.Overlay>, ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>>(
  ({ className, ...props }, ref) => (
    <DialogPrimitive.Overlay
      ref={ref}
      className={cn("ui-sheet-overlay", className)}
      {...props}
    />
  ),
);
SheetOverlay.displayName = DialogPrimitive.Overlay.displayName;

type SheetContentProps = ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
  side?: "top" | "bottom" | "left" | "right";
  closeDisabled?: boolean;
};

const SheetContent = forwardRef<ElementRef<typeof DialogPrimitive.Content>, SheetContentProps>(
  ({ className, children, side = "right", closeDisabled = false, ...props }, ref) => (
    <SheetPortal>
      <SheetOverlay />
      <DialogPrimitive.Content
        ref={ref}
        className={cn(
          "ui-sheet-content",
          side === "right" && "ui-sheet-content-right",
          side === "left" && "ui-sheet-content-left",
          side === "top" && "ui-sheet-content-top",
          side === "bottom" && "ui-sheet-content-bottom",
          className,
        )}
        {...props}
      >
        {children}
        <DialogPrimitive.Close asChild>
          <Button variant="ghost" size="icon" className="ui-sheet-close" aria-label="Close panel" disabled={closeDisabled}>
            <X size={16} />
          </Button>
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </SheetPortal>
  ),
);
SheetContent.displayName = DialogPrimitive.Content.displayName;

const SheetHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("ui-sheet-header", className)} {...props} />
);
SheetHeader.displayName = "SheetHeader";

const SheetTitle = forwardRef<ElementRef<typeof DialogPrimitive.Title>, ComponentPropsWithoutRef<typeof DialogPrimitive.Title>>(
  ({ className, ...props }, ref) => <DialogPrimitive.Title ref={ref} className={cn("ui-sheet-title", className)} {...props} />,
);
SheetTitle.displayName = DialogPrimitive.Title.displayName;

const SheetDescription = forwardRef<ElementRef<typeof DialogPrimitive.Description>, ComponentPropsWithoutRef<typeof DialogPrimitive.Description>>(
  ({ className, ...props }, ref) => <DialogPrimitive.Description ref={ref} className={cn("ui-sheet-description", className)} {...props} />,
);
SheetDescription.displayName = DialogPrimitive.Description.displayName;

const SheetBody = ({ className, ...props }: HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("ui-sheet-body", className)} {...props} />
);
SheetBody.displayName = "SheetBody";

const SheetFooter = ({ className, ...props }: HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("ui-sheet-footer", className)} {...props} />
);
SheetFooter.displayName = "SheetFooter";

export {
  Sheet,
  SheetTrigger,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetBody,
  SheetFooter,
  SheetTitle,
};
