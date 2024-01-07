import React from "react";
import { MAPBOX_STYLES } from "../config/mapbox-styles";
import { useMapConfigStore } from "../stores/map-config-store";

export const MapStylePicker = () => {
  const { mapStyle, setMapStyle } = useMapConfigStore((state) => ({
    mapStyle: state.mapStyle,
    setMapStyle: state.setMapStyle,
  }));

  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        flex: 1,
      }}
    >
      <select
        style={{
          flex: 1,
          borderRadius: "4px",
        }}
        className={"call-to-action"}
        value={mapStyle}
        onChange={(e) => setMapStyle(e.target.value)}
      >
        {MAPBOX_STYLES.map((style) => (
          <option key={style.name} value={style.url}>
            {style.name}
          </option>
        ))}
      </select>
    </div>
  );
};
