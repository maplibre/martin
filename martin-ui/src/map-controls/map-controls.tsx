import { CustomMapControl } from "../components/map-control/map-control";
import { GridIcon } from "../icons/grid";
import { PushpinFilled } from "@ant-design/icons";
import React from "react";
import { useModalStore } from "../stores/modal-store";
import { PositionPopup } from "../components/position-popup";
import { useMapConfigStore } from "../stores/map-config-store";
import {DatabaseIcon} from "../icons/database";

const ControlButton = ({
  onClick,
  children,
}: {
  onClick: () => void;
  children: React.ReactNode;
}) => {
  return (
    <button
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        cursor: "pointer",
        boxShadow: "0 6px 12px 0 rgba(0, 0, 0, 0.16)",
        height: "32px",
        width: "32px",
        padding: "2px",
      }}
    >
      {children}
    </button>
  );
};

export const MapControls = () => {
  const { openModal, setModalContent } = useModalStore((state) => ({
    openModal: state.actions.openModal,
    setModalContent: state.actions.setContent,
  }));

  const {
    showPosition,
    setShowPosition,
    showTilesBorders,
    setShowTilesBorders,
  } = useMapConfigStore((state) => ({
    showPosition: state.showPosition,
    setShowPosition: state.setShowPosition,
    setShowTilesBorders: state.setShowTilesBorders,
    showTilesBorders: state.showTilesBorders,
  }));

  return (
    <>
      <CustomMapControl position={"top-right"}>
        <>
          <div
            style={{
              position: "relative",
              zIndex: 999,
              pointerEvents: "all",
              padding: "8px",
              display: "flex",
              flexDirection: "column",
              gap: "8px",
            }}
          >
            <ControlButton
              onClick={() => setShowTilesBorders(!showTilesBorders)}
            >
              <GridIcon />
            </ControlButton>
            <ControlButton
              secondary
              onClick={() => {
                setModalContent("data-table");
                openModal();
              }}
            >
              <DatabaseIcon />
            </ControlButton>
            <ControlButton
              secondary
              onClick={() => setShowPosition(!showPosition)}
            >
              <PushpinFilled />
            </ControlButton>
          </div>
        </>
      </CustomMapControl>
      <CustomMapControl position={"bottom-right"}>
        <>{showPosition && <PositionPopup />}</>
      </CustomMapControl>
    </>
  );
};
