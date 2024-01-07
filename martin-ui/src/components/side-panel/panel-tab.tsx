import React from "react";
import styled from "styled-components";
import { Tooltip } from "@kepler.gl/components";
import { FormattedMessage } from "@kepler.gl/localization";
import { LayersIcon } from "../../icons/layers";
import { DatabaseFilled } from "@ant-design/icons";
import { useModalStore } from "../../stores/modal-store";
import { MapStylePicker } from "../map-style-picker";
import {useAppUiStore} from "../../stores/app-ui-store";
import {Button} from "../styled-components/button";

type StyledPanelTabProps = {
  active?: boolean;
};

export type PanelItem = {
  id: string;
  label: string;
  iconComponent: React.ComponentType;
};

export type PanelTabProps = {
  isActive: boolean;
  panel: PanelItem;
  onClick: (e: React.MouseEvent<HTMLDivElement>) => void;
};

export const StyledPanelTab = styled.div.attrs({
  className: "side-panel__tab",
})<StyledPanelTabProps>`
  align-items: flex-end;
  border-bottom-style: solid;
  border-bottom-width: 2px;
  border-bottom-color: ${(props) =>
    props.active ? props.theme.panelToggleBorderColor : "transparent"};
  color: ${(props) =>
    props.active ? props.theme.subtextColorActive : props.theme.panelTabColor};
  display: flex;
  justify-content: center;
  margin-right: ${(props) => props.theme.panelToggleMarginRight}px;
  padding-bottom: ${(props) => props.theme.panelToggleBottomPadding}px;
  width: ${(props) => props.theme.panelTabWidth};

  :hover {
    cursor: pointer;
    color: ${(props) => props.theme.textColorHl};
  }
`;

export const PanelTab: React.FC<PanelTabProps> = ({
  isActive,
  onClick,
  panel,
}) => (
  <StyledPanelTab
    data-tip
    data-for={`${panel.id}-nav`}
    active={isActive}
    onClick={onClick}
  >
    <panel.iconComponent height="20px" />
    <Tooltip
      id={`${panel.id}-nav`}
      effect="solid"
      delayShow={500}
      place="bottom"
    >
      <span>
        <FormattedMessage id={panel.label || panel.id} />
      </span>
    </Tooltip>
  </StyledPanelTab>
);

const PanelHeaderContainer = styled.div.attrs({
  className: "side-side-panel__header__bottom",
})`
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  width: 100%;
`;

const PanelTabsContainer = styled.div`
  display: flex;
  justify-content: flex-start;
  width: fit-content;
  margin-left: 10px;
  width: 100%;
`;

export const PanelHeader = ({
  panelTabs,
  activeId,
  onTabClick,
}: {
  panelTabs: PanelItem[];
  activeId: string;
  onTabClick: (item: PanelItem) => void;
}) => {
  const { closeSidePanel, openLoadData} = useAppUiStore((state) => ({
    closeSidePanel: state.actions.closeSidePanel,
    openLoadData: state.actions.openLoadData
  }));

  return (
    <PanelHeaderContainer className="side-side-panel__header__bottom">
      <div
        style={{
          display: "flex",
          gap: "8px",
        }}
      >
        <Button
          active
          onClick={() => {
            openLoadData();
            closeSidePanel();
          }}
          style={{
            width: "30px",
            height: "30px",
            margin: '0px 10px'
          }}
        >
          <DatabaseFilled />
        </Button>
        <MapStylePicker />
      </div>
      <PanelTabsContainer>
        {panelTabs.map((panel) => (
          <PanelTab
            key={panel.id}
            panel={panel}
            isActive={panel.id === activeId}
            onClick={() => onTabClick(panel)}
          />
        ))}
      </PanelTabsContainer>
    </PanelHeaderContainer>
  );
};
