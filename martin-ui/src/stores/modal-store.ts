import { create } from "zustand";
import { devtools } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";
import { enableMapSet } from "immer";

enableMapSet();

type ModalContentType = "data-table" | "load-data";

export type ModalStore = {
  open: boolean;
  content: ModalContentType;
  actions: {
    openModal: () => void;
    closeModal: () => void;
    setContent: (content: ModalContentType) => void;
  };
};

export const useModalStore = create<ModalStore>()(
  immer(
    devtools(
      (set, get) => ({
        open: false,
        content: "data-table",
        actions: {
          openModal: () => {
            set((state) => {
              console.log("openModal");
              state.open = true;
            });
          },
          closeModal: () => {
            set((state) => {
              state.open = false;
            });
          },
          setContent: (content: ModalContentType) => {
            set((state) => {
              state.content = content;
            });
          },
        },
      }),
      {
        name: "TileSetStore",
        enabled: false,
      },
    ),
  ),
);
