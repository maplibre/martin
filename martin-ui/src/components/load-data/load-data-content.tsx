import {useTileSetsStore} from "../../stores/tile-set-store";
import React from "react";
import {useAppUiStore} from "../../stores/app-ui-store";
import {Button} from "../styled-components/button";
import {TileSetPicker} from "../tile-set-picker";

export const LoadDataContent = () => {
  const selectedTileSets = useTileSetsStore((state) => state.selectedIds);
  const selectTileSet = useTileSetsStore(
    (state) => state.actions.selectTileSet,
  );
  const unselectTileSet = useTileSetsStore(
    (state) => state.actions.unselectTileSet,
  );
  const tileSets = useTileSetsStore((state) => state.tileSets);
  console.log('tileSets', tileSets);
  const { closeLoadData, openSidePanel } = useAppUiStore((state) => ({
    closeLoadData: state.actions.closeLoadData,
    openSidePanel: state.actions.openSidePanel,
  }));

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        width: "100%",
        height: "100%",
      }}
    >
      <div
        style={{
          width: "700px",
          height: "100%",
          paddingTop: "180px",
        }}
      >
        <div
          style={{
            position: "relative",
            textAlign: "center",
            fontSize: "45px",
            color: "rgb(115, 0, 255)",
            textShadow:
              "0 0 7px rgb(53, 28, 131),0 0 10px rgb(53, 28, 131),0 0 21px rgb(53, 28, 131),0 0 42px rgb(53, 28, 131),0 0 82px rgb(53, 28, 131),0 0 92px rgb(53, 28, 131),0 0 102px rgb(53, 28, 131),0 0 151px rgb(53, 28, 131)",
          }}
        >
          Martin
          <Button onClick={() => {
            closeLoadData();
            openSidePanel();
          }} style={{
            position: "absolute",
            top: "10px",
            right: "10px",
          }}>
            X
          </Button>
        </div>
        <div style={{
          height: "620px",
          overflowY: "scroll",
          overflowX: "hidden",
        }}>
        <TileSetPicker
          style={{
            overflowY: "scroll",
          }}
          tileSets={[...tileSets.values()]}
          onSelected={(tileSet) => selectTileSet(tileSet.id)}
          onRemove={(tileSet) => unselectTileSet(tileSet.id)}
          selectedTileSets={selectedTileSets}
        />
        </div>
      </div>
    </div>
  );
};