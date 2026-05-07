import type { ButtonHTMLAttributes, ForwardedRef } from "react";
import { forwardRef } from "react";
import { cn } from "../../utils/cn";

// shadcn/ui-compatible local wrapper. This repo keeps CSS tokens in globals.css
// instead of Tailwind classes, but preserves the shadcn component API shape.
const variantClasses: Record<string, string> = {
  default: "ui-button-default",
  secondary: "ui-button-secondary",
  ghost: "ui-button-ghost",
  outline: "ui-button-outline",
  destructive: "ui-button-destructive",
};

const sizeClasses: Record<string, string> = {
  default: "ui-button-default-size",
  sm: "ui-button-sm",
  lg: "ui-button-lg",
  icon: "ui-button-icon",
};

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: keyof typeof variantClasses;
  size?: keyof typeof sizeClasses;
};

export const Button = forwardRef(function Button(
  { className, variant = "default", size = "default", ...props }: ButtonProps,
  ref: ForwardedRef<HTMLButtonElement>,
) {
  return <button ref={ref} className={cn("ui-button", variantClasses[variant], sizeClasses[size], className)} {...props} />;
});
