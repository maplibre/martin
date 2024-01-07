import React  from "react";
import styled from "styled-components";
import "./app.css";
import { TileSetsMap } from "./tile-sets-map";
import { AppModal } from "./modal";
import { SidePanel } from "./side-panel/side-panel";
import {useAppUiStore} from "../stores/app-ui-store";
import {LoadDataScreen} from "./load-data/load-data-screen";

const AppContainer = styled.div`
  height: 100vh;
  display: flex;
  flex: auto;
  flex-direction: column;
  min-height: 0;
  background: #000000;
`;

const App: React.FC = () => {
  const { isLoadDataOpen, isSidePanelOpen } = useAppUiStore((state) => ({
    isLoadDataOpen: state.isLoadDataOpen,
    isSidePanelOpen: state.isSidePanelOpen,
    openLoadData: state.actions.openLoadData,
  }));

  return (
    <>
      <AppContainer>
        { isLoadDataOpen && <LoadDataScreen />}
        { isSidePanelOpen && <SidePanel/>}
        <TileSetsMap />
      </AppContainer>
      <AppModal />
    </>
  );
};

export default App;
