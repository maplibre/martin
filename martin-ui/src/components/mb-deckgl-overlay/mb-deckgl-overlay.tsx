import { usePopupStore } from "../../stores/popup-store";
import { useControl } from "react-map-gl/maplibre";
import { MapboxOverlay, MapboxOverlayProps } from "@deck.gl/mapbox/typed";
import Layer from "@deck.gl/core/lib/layer";
import {useMapConfigStore} from "../../stores/map-config-store";
import {useEffect} from "react";

export type MBDeckGLOverlayProps = MapboxOverlayProps & {
  interleaved?: boolean;
  layers: Layer<any>[];
};
export const MBDeckGLOverlay = (props: MBDeckGLOverlayProps) => {
  const { setPosition, setFeatures } = usePopupStore((state) => ({
    setPosition: state.actions.setPosition,
    setFeatures: state.actions.setFeatures,
  }));

  const { setDeckOverlayInstance } = useMapConfigStore((state) => ({
    setDeckOverlayInstance: state.setDeckOverlayInstance,
  }));

  const overlay = useControl<MapboxOverlay>(
    () =>
      new MapboxOverlay({
        ...props,
        onHover: (info) => {
          if (info.object) {
            setFeatures(info.layer!.id, [info.object]);
            info.coordinate && setPosition(info.coordinate as [number, number]);
          } else {
            setFeatures("", []);
          }
        },
      }),
  );

  useEffect(() => {
    setDeckOverlayInstance(overlay);
  }, [overlay, setDeckOverlayInstance]);

  overlay.setProps(props);
  return null;
};
