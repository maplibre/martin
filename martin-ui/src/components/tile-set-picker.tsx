import React from "react";
import "./tile-set-picker.scss";
import { TileSetCatalogItem } from "../types/tilesets";
import { useState } from "react";
import {Button} from "./styled-components/button";
import { PlusSquareIcon } from "../icons/plus-square";
import styled from "styled-components";
import JsonView from 'react18-json-view'
import 'react18-json-view/src/style.css'
import 'react18-json-view/src/dark.css'
import {TileJSON} from "../types/tile-json";
import {FileTextIcon} from "../icons/file-text-icon";
import {TileSet} from "../stores/tile-set-store";

interface TileSetPickerProps {
  tileSets: TileSet[];
  selectedTileSets: Set<string>;
  onSelected: (tileSet: TileSet) => void;
  onRemove: (tileSet: TileSet) => void;
}

const DataList = styled.div`
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 0 14px;
`;

const TileSetCard = styled.div`
  box-shadow:
    4px 6px 3px 3px rgba(0, 0, 0, 0.03),
    0 1px 6px -1px rgba(0, 0, 0, 0.02),
    0 2px 4px 0 rgba(0, 0, 0, 0.02);
  border: 1px solid lightgray;
  margin: 8px;
  width: 100%;
  border-radius: 8px;
  margin: 0;
  padding: 0;
  color: rgba(255, 255, 255, 0.85);
  font-size: 14px;
  line-height: 1.5714285714285714;
  list-style: none;
  font-family: -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,'Noto Sans',sans-serif,'Apple Color Emoji','Segoe UI Emoji','Segoe UI Symbol','Noto Color Emoji';
  position: relative;
  background: #141414; 
`;

const TileSetCardActions = styled.div`
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 8px;
  background: #141414;
  border-top: 1px solid lightgray;
  border-bottom-left-radius: 8px;
  border-bottom-right-radius: 8px;
  gap: 8px;
`;

const TileSetCardTitle = styled.div`
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px;
  background: #141414;
  border-top-left-radius: 8px;
  border-top-right-radius: 8px;
  font-size: 16px;
`;


export const TileSetPicker = ({
  tileSets,
  selectedTileSets,
  onSelected,
  onRemove,
}: TileSetPickerProps) => {
  const [showTileJson, setShowTileJson] = useState<TileJSON>(null);

  if (showTileJson) {
    return (
      <TileSetCard>
        <TileSetCardTitle>
          {showTileJson.name}
          <div onClick={() => {
            setShowTileJson(null);
          }}>
            Previous
          </div>
        </TileSetCardTitle>
        <JsonView src={showTileJson}/>
      </TileSetCard>
    );
  }

  console.log('tileSets', tileSets);
  return (
    <DataList>
      { tileSets.map((item) => {
        const isSelected = selectedTileSets.has(item.id);
        const action = (
          <Button
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
            active={isSelected}
            onClick={() => (isSelected ? onRemove(item) : onSelected(item))}
          >
            <PlusSquareIcon width={'14px'}/>
          </Button>
        );

        const showTileJsonButton = (
          <Button
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
            onClick={() => {
              setShowTileJson(item.tilejson);
            }}
          >
            <FileTextIcon width={'14px'} />
          </Button>
        );

        return (
          <TileSetCard key={item.id}>
            <TileSetCardTitle>{item.tilejson.name}</TileSetCardTitle>
            <TileSetCardActions>
              <div>{showTileJsonButton}</div>
              <div>{action}</div>
            </TileSetCardActions>
          </TileSetCard>
        )})}
    </DataList>
  );
};

interface CatalogNode {
  value?: string | TileSetCatalogItem;
  children: Record<string, CatalogNode>;
}
