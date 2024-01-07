import React from "react";
import { TileSetPicker } from "./tile-set-picker";
import { useTileSetsStore } from "../stores/tile-set-store";

export const Catalog: React.FC = () => {
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
        height: "100%",
      }}
    >
      <TileSetPicker
        tileSets={[...tileSets.values()]}
        onSelected={(tileSet) => selectTileSet(tileSet.id)}
        onRemove={(tileSet) => unselectTileSet(tileSet.id)}
        selectedTileSets={selectedTileSets}
      />
    </div>
  );
};
