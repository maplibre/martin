import React from "react";
import styled from "styled-components";
import { PanelHeader, PanelItem } from "./panel-tab";
import { LayersIcon } from "../../icons/layers";
import { TileSetsList } from "../tilesets-list";

const SidePanelContainer = styled.div`
  border-radius: 8px;
  z-index: 99;
  height: 100%;
  width: 270px;
  display: flex;
  transition: width 250ms ease 0s;
  flex-direction: column;
  background-color: rgb(36, 39, 48);
  border-radius: 1px;
  flex-direction: column;
  border-left: 0px solid transparent;
`;

const SidePanelHeader = styled.div`
  background-color: rgb(41, 50, 60);
  border-bottom: 1px solid transparent;
  padding: 4px 0px;
  display: flex;
  height: fit-content;
  width: 100%;
`;

const SidePanelContent = styled.div`
  padding: 20px 20px 30px;
  height: 100%;
`;

const PANEL_TABS: PanelItem[] = [
  {
    id: "layer",
    label: "Layer",
    iconComponent: LayersIcon,
    content: (
      <div
        style={{
          position: "relative",
          height: "100%",
          overflowY: "scroll",
        }}
      >
        <TileSetsList />
      </div>
    ),
  },
];

export const SidePanel: React.FC = () => {
  const [activePanel, setActivePanel] = React.useState<string>(
    PANEL_TABS[0].id,
  );

  return (
    <SidePanelContainer>
      <SidePanelHeader>
        <PanelHeader
          panelTabs={PANEL_TABS}
          activeId={activePanel}
          onTabClick={({ id }) => setActivePanel(id)}
        />
      </SidePanelHeader>
      <SidePanelContent>
        {PANEL_TABS.find(({ id }) => id === activePanel)?.content}
      </SidePanelContent>
    </SidePanelContainer>
  );
};
