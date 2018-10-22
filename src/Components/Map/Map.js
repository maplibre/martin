import React, { PureComponent } from 'react';
import mapboxgl from 'mapbox-gl';
import 'mapbox-gl/dist/mapbox-gl.css';

import { MAPBOX_STYLE, MAPBOX_TOKEN } from '../../config/constants';
import dateConverter from '../../utils/dateConverter';

import Container from './Container';
import Filters from './Filters';

const mapStyle = {
  height: '70vh',
  marginBottom: '95px'
};

class Map extends PureComponent {
  state = {
    range: {
      from: new Date(2017, 0, 1),
      to: new Date(2017, 4, 4)
    },
    hour: 9
  };

  componentDidMount() {
    mapboxgl.accessToken = MAPBOX_TOKEN;
    this.map = new mapboxgl.Map({
      container: 'map',
      style: MAPBOX_STYLE
    });
    this.nav = new mapboxgl.NavigationControl();

    this.map.scrollZoom.disable();
    this.map.addControl(this.nav, 'top-right');
    this.map.on('load', this.mapOnLoad);
  }

  componentDidUpdate() {
    const queryParams = this.getQueryParams();
    const newStyleUrl = `/tiles/rpc/public.get_trips.json?${queryParams}`;
    const newStyle = this.map.getStyle();

    newStyle.sources['public.get_trips'].url = newStyleUrl;
    this.map.setStyle(newStyle);
  }

  mapOnLoad = () => {
    const queryParams = this.getQueryParams();

    this.map.addSource('public.get_trips', {
      type: 'vector',
      url: `/tiles/rpc/public.get_trips.json?${queryParams}`
    });
    this.map.addLayer({
      id: 'trips',
      type: 'fill-extrusion',
      source: 'public.get_trips',
      'source-layer': 'trips',
      paint: {
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips'],
          0,
          10,
          3,
          30,
          70,
          70,
          90,
          150,
          300,
          300,
          2500,
          400,
          8000,
          600,
          12000,
          800,
          20000,
          1100,
          30000,
          1600,
          55000,
          2000,
          76000,
          3000
        ],
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips'],
          0,
          '#fff0f0',
          3,
          '#ffdade',
          70,
          '#ffc4d1',
          90,
          '#ffaec9',
          300,
          '#ff98c5',
          2500,
          '#f982c5',
          8000,
          '#ee6cc8',
          12000,
          '#de58ce',
          20000,
          '#c847d7',
          30000,
          '#ab3ae1',
          55000,
          '#8233ed',
          76000,
          '#3434f9'
        ],
        'fill-extrusion-opacity': 0.75
      }
    }, 'place-town');
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

  render() {
    const { range, hour } = this.state;

    return (
      <Container>
        <div
          id='map'
          style={mapStyle}
        />
        <Filters
          range={range}
          hour={hour}
          changeFilter={this.changeFilter}
        />
      </Container>
    );
  }
}

export default Map;
