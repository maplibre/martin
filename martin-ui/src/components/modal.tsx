import { useModalStore } from "../stores/modal-store";
import React, { useContext, useEffect } from "react";
import { RootContext } from "../main";
import {
  appInjector,
  DataTableModalFactory,
  ModalDialogFactory,
} from "@kepler.gl/components";
import { useTileSetsStore } from "../stores/tile-set-store";
import KeplerTable from "@kepler.gl/table";
import { useAppUiStore } from "../stores/app-ui-store";
import {useMapConfigStore} from "../stores/map-config-store";

const DataTableModal = appInjector.get(DataTableModalFactory);
const ModalDialog = appInjector.get(ModalDialogFactory);

export const AppModal = () => {
  const { openModal, modalIsOpen, closeModal, content } = useModalStore(
    (state) => ({
      closeModal: state.actions.closeModal,
      modalIsOpen: state.open,
      content: state.content,
    }),
  );

  const { toggleSidePanel, isSidePanelOpen } = useAppUiStore((state) => ({
    isSidePanelOpen: state.isSidePanelOpen,
    toggleSidePanel: state.actions.toggleSidePanel,
  }));

  const rootNode = useContext(RootContext);

  if (!rootNode?.current) {
    return null;
  }

  let modalContent = null;
  let modalTitle = "";
  switch (content) {
    case "data-table":
      modalContent = <DataTableModalContent />;
      break;
  }

  return (
    <ModalDialog
      cssStyle={{
        maxWidth: "100%",
        width: "100%",
        padding: "4px",
      }}
      title={modalTitle}
      parentSelector={() => rootNode?.current}
      isOpen={modalIsOpen}
      onCancel={() => {
        closeModal();
        toggleSidePanel();
      }}
    >
      {modalContent}
    </ModalDialog>
  );
};

export const DataTableModalContent = () => {
  const { tileSets } = useTileSetsStore((state) => ({
    tileSets: state.actions.getSelectedTileSets(),
  }));
  const {overlayInstance} = useMapConfigStore((state) => ({
    overlayInstance: state.deckOverlayInstance,
  }));


  const [dataId, setDataId] = React.useState<string | null>(
    tileSets[0]?.id ?? "",
  );
  const [keplerTables, setKeplerTable] = React.useState<KeplerTable[] | null>(
    null,
  );
  useEffect(() => {
    if (!overlayInstance) return;
    //
    // const tables = tileSets?.map((tileSet) => {
    //   const featureCollection = {
    //     type: "FeatureCollection",
    //     features: renderedFeatures,
    //   };
    //   const keplerTableData = processGeojson(featureCollection);
    //   const { visConfig, tilejson } = tileSet;
    //   return new KeplerTable({
    //     data: keplerTableData as ProtoDataset["data"],
    //     color: visConfig?.color ?? colorMaker.next().value,
    //     info: {
    //       id: tileSet.id,
    //       label: tilejson.name,
    //     },
    //   });
    // });
    //
    // const datasets = {};
    // tables.forEach((table) => {
    //   datasets[table.id] = table;
    // });
    // setKeplerTable(datasets);
  }, [overlayInstance]);

  if (!Object.keys(keplerTables ?? {})?.length > 0) {
    return <div>Loading...</div>;
  }

  return (
    <DataTableModal
      dataId={dataId}
      datasets={keplerTables}
      showDatasetTable={(dataId) => {
        setDataId(dataId);
      }}
      showTab
    />
  );
};
