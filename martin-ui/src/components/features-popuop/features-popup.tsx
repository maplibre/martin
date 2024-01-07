import React from "react";
import { Popup } from "react-map-gl/maplibre";
import "./features-popup.scss";
import { usePopupStore } from "../../stores/popup-store";
import { useTileSetsStore } from "../../stores/tile-set-store";

export const FeaturesPopup = ({ ...popupProps }) => {
  const { features, position, id } = usePopupStore((state) => ({
    features: state.features,
    position: state.position,
    id: state.layerId,
  }));

  const { layer } = useTileSetsStore((state) => ({
    layer: state.tileSets.get(id!),
  }));

  if (
    !features ||
    !(features?.length > 0) ||
    !(position?.length === 2) ||
    !layer
  ) {
    return null;
  }

  return (
    <Popup
      style={{
        zIndex: 1000,
        backgroundColor: 'transparent',
      }}
      longitude={position[0]}
      latitude={position[1]}
      closeOnMove={false}
      closeOnClick={false}
      {...popupProps}
    >
      <>
        {features.map((feature) => {
          return (
            <div key={feature.id} className={"features-popup"}>
              <div className={"layer-name"}>{layer.tilejson.name}</div>
              <div className={"feature-properties"}>
                {Object.keys(feature.properties ?? {}).map((key) => {
                  return (
                    <>
                      <span className={"feature-property-key"}>{key}:</span>
                      <span className={"feature-property-value"}>
                        {JSON.stringify(feature.properties?.[key])}
                      </span>
                    </>
                  );
                })}
              </div>
            </div>
          );
        })}
      </>
    </Popup>
  );
};
