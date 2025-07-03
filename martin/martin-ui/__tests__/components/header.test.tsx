import { screen } from "@testing-library/react";
import React from "react";
import { Header } from "@/components/header";
import { render } from "../utils/test-utils";

jest.mock("@/components/theme-switcher", () => ({
  ThemeSwitcher: () => <div data-testid="theme-switcher">Theme Switcher Mock</div>
}));
jest.mock("next/image", () => ({
  __esModule: true,
  default: (props: any) => {
    // Convert boolean props to strings to avoid React DOM warnings
    const imgProps = {...props};
    if (typeof imgProps.priority === 'boolean') {
      imgProps.priority = imgProps.priority.toString();
    }
    return <img {...imgProps} />;
  }
}));

// Set environment variable used in Header
process.env.NEXT_PUBLIC_VERSION = 'v0.0.0-test';

describe("Header Component", () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it("renders correctly", () => {
    const { container } = render(<Header />);
    expect(container).toMatchSnapshot();
  });

  it("displays the Martin version", () => {
    const { getByText } = render(<Header />);
    expect(getByText("v0.0.0-test")).toBeInTheDocument();
  });

  it("contains navigation links", () => {
    const { getByText } = render(<Header />);

    const documentationLink = getByText("Documentation");
    expect(documentationLink).toBeInTheDocument();

    // hidden on mobile, so using a more specific query
    const aboutUsLink = getByText("About us");
    expect(aboutUsLink).toBeInTheDocument();
    expect(aboutUsLink.closest("a")).toHaveAttribute("href", "https://maplibre.org");
  });

  it("includes the theme switcher", () => {
    const { getByTestId } = render(<Header />);
    expect(getByTestId("theme-switcher")).toBeInTheDocument();
  });
});
