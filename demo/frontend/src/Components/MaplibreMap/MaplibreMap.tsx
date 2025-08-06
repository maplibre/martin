/* biome-ignore-all lint/suspicious/noExplicitAny: this is a legacy component and needs to be redone */
import maplibregl from "maplibre-gl";
import { useCallback, useEffect, useRef, useState } from "react";
import "maplibre-gl/dist/maplibre-gl.css";

import { MAP_STYLE } from "../../config/constants";
import layers from "../../config/layers";

import Container from "./Container";
import Filters from "./Filters";
import type { DateRange } from "react-day-picker";

const mapStyle = { height: "615px", marginLeft: "350px" };

const MaplibreMap = () => {
	const mapRef = useRef<any>(null);
	const navRef = useRef<any>(null);
	const mapContainerRef = useRef<HTMLDivElement>(null);

	const [hourFilter, setHourFilter] = useState(7);
	const [rangeFilter, setRangeFilter] = useState<DateRange>({
		from: new Date(2017, 0, 1),
		to: new Date(2017, 4, 4),
	});
	const [visibleLayer, setVisibleLayer] = useState("trips");

	const getQueryParams = useCallback(() => {
		const dateFrom = `${rangeFilter.from.toLocaleDateString()}.2017`;
		const dateTo = rangeFilter.to !== undefined ? `${rangeFilter.to.toLocaleDateString()}.2017` : dateFrom;

		return encodeURI(`date_from=${dateFrom}&date_to=${dateTo}&hour=${hourFilter}`);
	}, [rangeFilter, hourFilter]);

	const mapOnLoad = useCallback(() => {
		const queryParams = getQueryParams();

		mapRef.current.addSource("trips_source", {
			type: "vector",
			url: `/tiles/get_trips?${queryParams}`,
		});
		layers.forEach(({ maplibreLayer }) => {
			mapRef.current.addLayer(maplibreLayer, "place_town");
		});
	}, [getQueryParams]);

	const toggleLayer = (layerId: string) => {
		layers.forEach(({ id }) => {
			if (layerId === id) {
				mapRef.current.setLayoutProperty(id, "visibility", "visible");
			} else {
				mapRef.current.setLayoutProperty(id, "visibility", "none");
			}
		});
		setVisibleLayer(layerId);
	};

	// Initialize map on mount
	useEffect(() => {
		if (!mapContainerRef.current) return;

		mapRef.current = new maplibregl.Map({
			center: [-74.005308, 40.71337],
			container: mapContainerRef.current,
			cooperativeGestures: true,
			pitch: 45,
			style: MAP_STYLE,
			zoom: 9,
		});

		navRef.current = new maplibregl.NavigationControl();
		mapRef.current.addControl(navRef.current, "top-right");
		mapRef.current.on("load", mapOnLoad);

		// Cleanup function
		return () => {
			if (mapRef.current) {
				mapRef.current.remove();
			}
		};
	}, [mapOnLoad]);

	// Update map when state changes (equivalent to componentDidUpdate)
	useEffect(() => {
		if (!mapRef.current || !mapRef.current.isStyleLoaded()) return;

		const newStyle = mapRef.current.getStyle();
		if (newStyle?.sources?.trips_source) {
			newStyle.sources.trips_source.url = `/tiles/get_trips?${getQueryParams()}`;
			mapRef.current.setStyle(newStyle);
		}
	}, [getQueryParams]);

	return (
		<Container>
			<Filters
				changeRangeFilter={(value)=>setRangeFilter(value)}
				changeHourFilter={(value: number) => setHourFilter(value)}
				hour={hourFilter}
				range={rangeFilter}
				toggleLayer={toggleLayer}
				visibleLayer={visibleLayer}
			/>
			<div ref={mapContainerRef} style={mapStyle} />
		</Container>
	);
};

export default MaplibreMap;
