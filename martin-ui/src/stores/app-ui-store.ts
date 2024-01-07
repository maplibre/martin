import { create } from "zustand";
import { devtools } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";
import { enableMapSet } from "immer";
enableMapSet();
export type AppUiStore = {
  isSidePanelOpen: boolean;
  isLoadDataOpen: boolean;
  actions: {
    openLoadData: () => void;
    closeLoadData: () => void;
    openSidePanel: () => void;
    closeSidePanel: () => void;
  };
};

export const useAppUiStore = create<AppUiStore>()(
  immer(
    devtools(
      (set, get) => ({
        isSidePanelOpen: false,
        isLoadDataOpen: true,
        actions: {
          openSidePanel: () => {
            set((state) => {
              state.isSidePanelOpen = true;
            });
          },
          closeSidePanel: () => {
            set((state) => {
              state.isSidePanelOpen = false;
            });
          },
          openLoadData: () => {
            set((state) => {
              state.isLoadDataOpen = true;
            });
          },
          closeLoadData: () => {
            set((state) => {
              state.isLoadDataOpen = false;
            });
          }
        },
      }),
      {
        name: "AppUiStore",
        enabled: false,
      },
    ),
  ),
);
