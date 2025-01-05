import React, { PureComponent } from 'react';
import maplibregl from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';

import { MAP_STYLE } from '../../config/constants';
import layers from '../../config/layers';
import dateConverter from '../../utils/dateConverter';

import Container from './Container';
import Filters from './Filters';

const mapStyle = { height: '615px', marginLeft: '350px' };

class Map extends PureComponent<{}, {visibleLayer: any, range: any, hour: any}> {

  map: any;
  nav: any;

  constructor(props: {} | Readonly<{}>) {
    super(props);
    this.state = {
      visibleLayer: 'trips',
      range: {
        from: new Date(2017, 0, 1),
        to: new Date(2017, 4, 4)
      },
      hour: 9
    };
  }

  componentDidMount() {
    this.map = new maplibregl.Map({
      cooperativeGestures: true,
      container: 'map',
      style: MAP_STYLE,
      center: [-74.005308, 40.713370],
      pitch: 45,
      zoom: 9,
    });
    this.nav = new maplibregl.NavigationControl();

    this.map.addControl(this.nav, 'top-right');
    this.map.on('load', this.mapOnLoad);
  }

  componentDidUpdate() {
    const newStyle = this.map.getStyle();
    newStyle.sources['trips_source'].url = `/tiles/get_trips?${this.getQueryParams()}`;
    this.map.setStyle(newStyle);
  }

  mapOnLoad = () => {
    const queryParams = this.getQueryParams();

    this.map.addSource('trips_source', {
      type: 'vector',
      url: `/tiles/get_trips?${queryParams}`
    });
    layers.forEach(({ maplibreLayer }) => {
      this.map.addLayer(maplibreLayer, 'place_town');
    });
  };

  changeFilter = (filter: string, value: any) => {
    if (filter !== undefined && value !== undefined) {
      this.setState(state => ({
        ...state,
        [filter]: value
      }));
    }
  };

  getQueryParams = () => {
    const { range: { from, to }, hour } = this.state;

    const dateFrom = `${dateConverter(from)}.2017`;
    let dateTo = `${dateConverter(to)}.2017`;
    if (to === undefined) {
      dateTo = dateFrom;
    }

    return encodeURI(`date_from=${dateFrom}&date_to=${dateTo}&hour=${hour}`);
  };

  toggleLayer = (layerId: string) => {
    layers.forEach(({ id }) => {
      if (layerId === id) {
        this.map.setLayoutProperty(id, 'visibility', 'visible');
      } else {
        this.map.setLayoutProperty(id, 'visibility', 'none');
      }
    });
    this.setState({ visibleLayer: layerId });
  };

  render() {
    const { visibleLayer, range, hour } = this.state;

    return (
      <Container>
        <Filters
          visibleLayer={visibleLayer}
          range={range}
          hour={hour}
          toggleLayer={this.toggleLayer}
          changeFilter={this.changeFilter}
        />
        <div
          id='map'
          style={mapStyle}
        />
      </Container>
    );
  }
}

export default Map;
