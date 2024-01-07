import React, { useState } from "react";
import { PlusSquareIcon } from "../icons/plus-square";
import { TileSetsList } from "./tilesets-list";
import { TileSetPicker } from "./tile-set-picker";
import { MapStylePicker } from "./map-style-picker";
import { useTileSetsStore } from "../stores/tile-set-store";

export const Sidebar = () => {
  const [open, setOpen] = useState(false);
  const handleOpen = () => setOpen(true);
  const handleClose = () => setOpen(false);
  const selectedTileSets = useTileSetsStore((state) => state.selectedIds);
  const selectTileSet = useTileSetsStore(
    (state) => state.actions.selectTileSet,
  );
  const unselectTileSet = useTileSetsStore(
    (state) => state.actions.unselectTileSet,
  );
  const tileSets = useTileSetsStore((state) => state.tileSets);
  return (
    <div
      style={{
        zIndex: 999,
        width: "300px",
        display: "flex",
        flexDirection: "row",
      }}
    >
      <div
        id="side-bar-controller"
        style={{
          width: "100px",
          height: "100%",
          backgroundColor: "rgba(0,0,0,0.2)",
        }}
      ></div>
      <div
        id="side-bar-content"
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "start",
          justifyContent: "start",
          overflow: "hidden",
          width: "100%",
          padding: "4px",
        }}
      >
        <div
          style={{
            width: "100%",
          }}
        >
          <div
            className={"sub-row"}
            style={{
              justifyContent: "space-between",
              display: "flex",
              alignItems: "center",
            }}
          >
            <MapStylePicker />
            <span
              onClick={open ? handleClose : handleOpen}
              style={{
                cursor: "pointer",
              }}
            >
              <PlusSquareIcon />
            </span>
          </div>
        </div>
        <hr />
        <TileSetsList />
        <div
          style={{
            position: "absolute",
            width: "330px",
            top: "0",
            left: "300px",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            display: open ? "flex" : "none",
          }}
        >
          <TileSetPicker
            tileSets={[...tileSets.values()]}
            onSelected={(tileSet) => selectTileSet(tileSet.id)}
            onRemove={(tileSet) => unselectTileSet(tileSet.id)}
            selectedTileSets={selectedTileSets}
          />
          <button
            onClick={handleClose}
            className={"icon-button"}
            style={{
              position: "absolute",
              top: "0",
              right: "0",
              padding: "4px",
            }}
          >
            X
          </button>
        </div>
      </div>
    </div>
  );
};
