import React  from "react";
import "./tilesets-list.css";
import {
  DragDropContext,
  Draggable,
  Droppable,
  OnDragEndResponder,
} from "react-beautiful-dnd";
import { LayersIcon } from "../icons/layers";
import { TrashIcon } from "../icons/trash";
import { ThreeRowsIcon } from "../icons/three-rows";
import { TileSet, useTileSetsStore } from "../stores/tile-set-store";

interface TileSetListItemProps {
  tileSet: TileSet;
}

const TileSetListItem = ({ tileSet }: TileSetListItemProps) => {
  const { unselect } = useTileSetsStore((state) => ({
    unselect: () => state.actions.unselectTileSet(tileSet.id),
  }));

  const tilejson = tileSet.tilejson;

  return (
    <div className={"txt-xs text-bold"}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          color: "white",
          border: "1px solid lightgrey",
          gap: "4px",
          padding: "4px",
          backgroundColor: "#627889",
        }}
      >
        <div
          style={{
            marginRight: "4px",
            borderRight: "1px solid lightgrey",
            paddingRight: "4px",
            display: "flex",
            alignItems: "center",
          }}
        >
          <ThreeRowsIcon />
        </div>
        <LayersIcon />
        <div
          style={{
            flex: 1,
          }}
        >
          {tilejson.name}
        </div>
        <button className={"remove-tile-set-button"} onClick={unselect}>
          <TrashIcon />
        </button>
      </div>
      {tilejson.vector_layers && (
        <div>
          {/*<>*/}
          {/*{tilejson.vector_layers.map((layer) => (*/}
          {/*  // <VectorLayerConfigurator*/}
          {/*  //   key={layer.id}*/}
          {/*    // layerConfig={layersConfig[layer.id] as FillLayer}*/}
          {/*    // onLayerConfigChange={(layerConfig) =>*/}
          {/*    //   setLayerConfig(`${layer.id}`, layerConfig)*/}
          {/*    // }*/}
          {/*  // />*/}
          {/*// ))}*/}
          {/*</>*/}
        </div>
      )}
    </div>
  );
};

export const TileSetsList = () => {
  const selectedTileSets = useTileSetsStore((state) =>
    state.actions.getSelectedTileSets(),
  );
  const setSelectedTileSetsIds = useTileSetsStore(
    (state) => state.actions.setSelectedTileSetsIds,
  );

  const onDragEnd: OnDragEndResponder = (result) => {
    if (!result.destination) {
      return;
    }
    const items = Array.from(selectedTileSets);
    const [reorderedItem] = items.splice(result.source.index, 1);
    items.splice(result.destination.index, 0, reorderedItem);
    setSelectedTileSetsIds(items.map((item) => item.id));
  };

  return (
    <DragDropContext onDragEnd={onDragEnd} className={"tile-sets-list"}>
      <Droppable droppableId="droppable">
        {(provided) => (
          <div
            className={"tile-sets-list"}
            ref={provided.innerRef}
            {...provided.droppableProps}
          >
            {selectedTileSets?.map((item, index) => (
              <Draggable key={item.id} index={index} draggableId={item.id}>
                {(provided) => (
                  <div
                    ref={provided.innerRef}
                    {...provided.draggableProps}
                    {...provided.dragHandleProps}
                  >
                    <TileSetListItem tileSet={item} />
                  </div>
                )}
              </Draggable>
            ))}
          </div>
        )}
      </Droppable>
    </DragDropContext>
  );
};
