import React, {useEffect} from "react";
import {useTileSetsStore} from "../../stores/tile-set-store";
import {LoadDataContent} from "./load-data-content";
import {PuffLoader} from "react-spinners";

export const LoadDataScreen = () => {
  const [loaded, setLoaded] = React.useState(false);
  const initCatalog = useTileSetsStore((state) => state.actions.initCatalog);
  const tileSets = useTileSetsStore((state) => state.tileSets);

  useEffect(() => {
    const init = async () => {
      if (!(tileSets.size > 0)) await initCatalog();
      setLoaded(true);
    }

    void init();
  }, [initCatalog]);

  return (
    <div style={{
        zIndex: 9999999,
        height: "100%",
      }}
    >
      { loaded && <LoadDataContent />}
      { !loaded &&
        <div style={{
          width: "100%",
          height: "100%",
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
        }}>
          <PuffLoader
            color="#5201ff"
            loading
            size={100}
            speedMultiplier={0.5}
          />
      </div>}
    </div>
  );
};
