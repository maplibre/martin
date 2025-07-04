import React from "react";
import { LoadingSpinner } from "@/components/loading/loading-spinner";
import { render } from "../../test-utils";

describe("LoadingSpinner Component", () => {
  it("renders correctly with default size", () => {
    const { container } = render(<LoadingSpinner />);
    expect(container).toMatchSnapshot();
  });

  it("renders correctly with small size", () => {
    const { container } = render(<LoadingSpinner size="sm" />);
    expect(container).toMatchSnapshot();
  });

  it("renders correctly with large size", () => {
    const { container } = render(<LoadingSpinner size="lg" />);
    expect(container).toMatchSnapshot();
  });

  it("applies additional classes", () => {
    const { container } = render(<LoadingSpinner className="text-red-500" />);
    expect(container).toMatchSnapshot();
  });
});
