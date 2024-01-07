import React from "react";
import classNames from "classnames";
import "./color-indicator.scss";

interface ColorIndicatorProps {
  color?: string;
  className?: string;
}

export const ColorIndicator = ({
  color = "lightgrey",
  className,
}: ColorIndicatorProps) => {
  return (
    <div
      className={classNames("color-indicator", className)}
      style={{
        backgroundColor: color,
      }}
    />
  );
};
