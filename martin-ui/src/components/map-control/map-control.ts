import { cloneElement, ReactElement } from "react";
import { createPortal } from "react-dom";
import { useControl, IControl } from "react-map-gl/maplibre";
import type { MapboxMap, ControlPosition } from "react-map-gl";

class MapControl implements IControl {
  _map: MapboxMap | null = null;
  _container: HTMLElement = document.createElement("div");

  constructor() {
    this._container.setAttribute(
      "style",
      'position: relative; bottom: 0; left: 0; z-index: 1000; pointerEvents: "all"',
    );
  }

  onAdd(map: MapboxMap) {
    this._map = map;
    return this._container;
  }

  onRemove() {
    this._container.remove();
    this._map = null;
  }

  getElement() {
    return this._container;
  }

  getDefaultPosition() {
    return "bottom-left";
  }
}
export interface CustomMapControlProps {
  children: ReactElement;
  position: ControlPosition | undefined;
}

export const CustomMapControl = (props: CustomMapControlProps) => {
  const ctrl = useControl(() => new MapControl(), {
    position: props.position,
  });

  return createPortal(cloneElement(props.children), ctrl.getElement());
};
