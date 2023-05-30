import React, { PureComponent } from 'react';
import maplibregl from 'maplibre-gl';
import 'maplibre-gl/dist/maplibre-gl.css';

import { MAP_STYLE } from '../../config/constants';
import layers from '../../config/layers';
import dateConverter from '../../utils/dateConverter';

import Container from './Container';
import Filters from './Filters';

const mapStyle = { height: '615px', marginLeft: '350px' };

class Map extends PureComponent<{}, {visibleLayer, range, hour}> {

  map: any;
  nav: any;

  constructor(props) {
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
      container: 'map',
      style: MAP_STYLE,
      center: [-74.005308, 40.713370],
      pitch: 45,
      zoom: 9,
    });
    this.nav = new maplibregl.NavigationControl();

    this.map.scrollZoom.disable();
    this.map.addControl(this.nav, 'top-right');
    this.map.on('load', this.mapOnLoad);
  }

  componentDidUpdate() {
    const queryParams = this.getQueryParams();
    const newStyleUrl = `http://localhost:3000/get_trips?${queryParams}`;
    const newStyle = this.map.getStyle();

    newStyle.sources['public.get_trips'].url = newStyleUrl;
    this.map.setStyle(newStyle);
  }

  mapOnLoad = () => {
    const queryParams = this.getQueryParams();

    this.map.addSource('public.get_trips', {
      type: 'vector',
      url: `http://localhost:3000/get_trips?${queryParams}`
    });
    layers.forEach(({ mapboxLayer }) => {
      this.map.addLayer(mapboxLayer, 'place_town');
    });
  };

  changeFilter = (filter, value) => {
    this.setState(state => ({
      ...state,
      [filter]: value
    }));
  };

  getQueryParams = () => {
    const { range: { from, to }, hour } = this.state;

    const dateFrom = `${dateConverter(from)}.2017`;
    const dateTo = `${dateConverter(to)}.2017`;

    return encodeURI(`date_from=${dateFrom}&date_to=${dateTo}&hour=${hour}`);
  };

  toggleLayer = (layerId) => {
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
