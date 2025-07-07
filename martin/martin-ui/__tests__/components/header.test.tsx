import { screen } from "@testing-library/react";
import { Header } from "@/components/header";
import { render } from "../test-utils";

// No need to mock next/image since we're using Vite and the Logo component is SVG

// Mock import.meta.env for tests
const mockImportMeta = {
  env: {
    VITE_MARTIN_VERSION: "v0.0.0-test",
  },
};

// @ts-ignore
global.import = { meta: mockImportMeta };

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
    render(<Header />);
    // Look for the theme switcher button by its aria-label
    const themeSwitcher = screen.getByRole("button", { name: /switch to.*theme/i });
    expect(themeSwitcher).toBeInTheDocument();
  });
});
