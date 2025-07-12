import { ThemeProvider } from "next-themes";
import { Header } from "@/components/header";
import { TooltipProvider } from "@/components/ui/tooltip";
import MartinTileserverDashboard from "./Dashboard";

function App() {
  return (
    <div className="font-sans">
      <ThemeProvider attribute="class" defaultTheme="system" disableTransitionOnChange enableSystem>
        <TooltipProvider>
          <div className="min-h-screen bg-background">
            <Header />
            <MartinTileserverDashboard />
          </div>
        </TooltipProvider>
      </ThemeProvider>
    </div>
  );
}

export default App;
