import type { ComponentPropsWithoutRef } from "react";

import { accentClasses, useAccent } from "./accent";
import { cx } from "./cx";

export type InlineAlertTone = "warning" | "danger" | "success" | "info";

export interface InlineAlertProps extends Omit<ComponentPropsWithoutRef<"div">, "role"> {
  tone: InlineAlertTone;
}

const looks: Record<InlineAlertTone, string> = {
  warning: "border-warning/30 bg-warning/10 text-warning-text",
  danger: "border-danger/30 bg-danger/10 text-danger-text",
  success: "border-success-soft/40 bg-success-soft/10 text-success-text",
  info: "border-info/30 bg-info/10 text-info-text",
};

// Warnings and errors interrupt (alert); success and info are polite (status), as the app does today.
const roles: Record<InlineAlertTone, "alert" | "status"> = {
  warning: "alert",
  danger: "alert",
  success: "status",
  info: "status",
};

export function InlineAlert({ tone, className, ...rest }: InlineAlertProps) {
  const accent = useAccent();
  // In a connection's panel, success and info messages wear the connection's colour. Warning and danger never change.
  const look = accent && (tone === "success" || tone === "info") ? accentClasses[accent].banner : looks[tone];
  return (
    <div
      role={roles[tone]}
      data-tone={tone}
      className={cx("rounded-ds-lg border px-3 py-2 font-type-body-md text-type-body-md text-shadow-label", look, className)}
      {...rest}
    />
  );
}
