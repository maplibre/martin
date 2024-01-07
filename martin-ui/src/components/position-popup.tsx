import { useMap } from "react-map-gl/maplibre";
import React, { useEffect, useState } from "react";

export interface PositionPopupProps {
  center: [string, string];
  zoom: string;
  pitch: string;
  bearing: string;
}

export const PositionPopup = () => {
  const map = useMap();
  const [position, setPosition] = useState<PositionPopupProps>(
    {} as PositionPopupProps,
  );

  useEffect(() => {
    if (map?.current) updatePosition();
  }, []);

  if (!map?.current) return null;

  map.current.on("moveend", function () {
    updatePosition();
  });

  const updatePosition = function () {
    let mapInstance = map.current;
    if (!mapInstance) return;
    const { lng, lat } = mapInstance.getCenter();
    setPosition({
      center: [lng.toFixed(5), lat.toFixed(5)],
      zoom: mapInstance.getZoom().toFixed(2),
      pitch: mapInstance.getPitch().toFixed(2),
      bearing: mapInstance.getBearing().toFixed(2),
    });
  };

  return (
    <div
      style={{
        margin: "10px",
        padding: "10px",
        background: "white",
        fontSize: "11px",
        zIndex: 1,
        lineHeight: "18px",
        fontFamily: "Open Sans, sans-serif",
        display: "flex",
        flexDirection: "column",
        alignItems: "start",
        borderRadius: "4px",
        boxShadow: "0 1px 2px rgba(0,0,0,0.1)",
      }}
    >
      <div>Center: {position.center?.join(", ")}</div>
      <div>Zoom: {position.zoom}</div>
      <div>Pitch: {position.pitch}</div>
      <div>Bearing: {position.bearing}</div>
    </div>
  );
};
