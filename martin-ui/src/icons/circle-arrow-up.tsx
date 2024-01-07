import React, { SVGProps } from "react";

export type CircleArrowUpProps = {
  className?: string;
} & SVGProps<SVGSVGElement>;

export default function CircleArrowUp(props: CircleArrowUpProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="feather feather-arrow-up-circle"
      {...props}
    >
      <circle cx="12" cy="12" r="10"></circle>
      <polyline points="16 12 12 8 8 12"></polyline>
      <line x1="12" y1="16" x2="12" y2="8"></line>
    </svg>
  );
}
