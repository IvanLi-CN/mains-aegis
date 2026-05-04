import { cn } from "../../utils/cn";

export type SegmentedControlOption<T extends string> = readonly [value: T, label: string];

type SegmentedControlProps<T extends string> = {
  label: string;
  value: T;
  options: ReadonlyArray<SegmentedControlOption<T>>;
  onChange: (value: T) => void;
  className?: string;
  variant?: "default" | "compact" | "quiet";
  getOptionTitle?: (value: T) => string;
};

export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
  variant = "default",
  getOptionTitle,
}: SegmentedControlProps<T>) {
  return (
    <div className={cn("ui-segmented-control", variant !== "default" && `is-${variant}`, className)} aria-label={label}>
      {options.map(([optionValue, optionLabel]) => (
        <button
          key={optionValue}
          className={value === optionValue ? "is-active" : ""}
          type="button"
          aria-pressed={value === optionValue}
          title={getOptionTitle?.(optionValue)}
          onClick={() => onChange(optionValue)}
        >
          {optionLabel}
        </button>
      ))}
    </div>
  );
}
