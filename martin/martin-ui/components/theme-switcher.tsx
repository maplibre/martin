"use client";

import { Moon, Sun, SunMoon } from "lucide-react";
import { useTheme } from "next-themes";

import { Button } from "./ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

const themeOrder = ["light", "dark", "system"] as const;

export function ThemeSwitcher() {
  const { theme, setTheme } = useTheme();

  const getNextTheme = () => {
    const idx = themeOrder.indexOf((theme as (typeof themeOrder)[number]) || "system");
    return themeOrder[(idx + 1) % themeOrder.length];
  };

  const getIcon = () => {
    switch (theme) {
      case "light":
        return <Sun aria-hidden="true" className="h-[1.2rem] w-[1.2rem]" />;
      case "dark":
        return <Moon aria-hidden="true" className="h-[1.2rem] w-[1.2rem]" />;
      default:
        return <SunMoon aria-hidden="true" className="h-[1.2rem] w-[1.2rem]" />;
    }
  };

  const getLabel = () => {
    switch (theme) {
      case "light":
        return "Switch to dark theme";
      case "dark":
        return "Switch to system theme";
      default:
        return "Switch to light theme";
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={getLabel()}
          onClick={() => setTheme(getNextTheme())}
          size="icon"
          variant="outline"
        >
          {getIcon()}
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <p>{getLabel()}</p>
      </TooltipContent>
    </Tooltip>
  );
}
