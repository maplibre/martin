import React, { PureComponent } from 'react';
import mapboxgl from 'mapbox-gl';
import { DateUtils } from 'react-day-picker';
import 'mapbox-gl/dist/mapbox-gl.css'
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
      to: new Date(2017, 4, 4),
    },
    hour: 9
  };

  componentDidMount() {
    mapboxgl.accessToken = 'pk.eyJ1IjoibWFydGlucHJvamVjdDEiLCJhIjoiY2ptdW93MXZrMDNjMTNrcGhmNTJ1ZGljdCJ9.9fC5LXUepNAYTKu8O162OA';
    this.map = new mapboxgl.Map({
      container: 'map',
      style: 'mapbox://styles/martinproject1/cjnfxj6053wz32rq8r9sija4o'
    });
    this.nav = new mapboxgl.NavigationControl();
    this.map.scrollZoom.disable();
    this.map.addControl(this.nav, 'top-right');
    this.map.on('load', this.mapOnLoad);
  }

  componentDidUpdate() {
    const { range, hour } = this.state;
    const { from, to } = range;
    if (!from || !to) return;

    const dateFrom = this.dateConverter(from);
    const dateTo = this.dateConverter(to);
    const queryParams = encodeURI(`date_from=${dateFrom}&date_to=${dateTo}&hour=${hour}`);

    const newStyle = this.map.getStyle();
    newStyle.sources['public.get_trips'].url = `/tiles/rpc/public.get_trips.json?${queryParams}`;
    this.map.setStyle(newStyle);
  }

  mapOnLoad = () => {
    this.map.addSource('public.get_trips', {
      type: 'vector',
      url: '/tiles/rpc/public.get_trips.json?date_from=01.01.2017&date_to=02.01.2017&hour=9'
    });
    this.map.addLayer({
      id: 'trips',
      type: 'fill-extrusion',
      source: 'public.get_trips',
      'source-layer': 'trips',
      paint: {
        "fill-extrusion-height": [
          "interpolate",
          ["exponential", 1.3],
          ["get", "trips"],
          17,
          10,
          1204,
          100,
          2526,
          200,
          4738,
          400,
          6249,
          600
        ],
        "fill-extrusion-color": [
          "interpolate",
          ["exponential", 1.3],
          ["get", "trips"],
          0,
          "#f2a8ff",
          2,
          "#f2a8ff",
          15,
          "#dc70ff",
          26,
          "#bc39fe",
          540,
          "#9202fd",
          900,
          "#6002c5"
        ],
        "fill-extrusion-opacity": 0.75
      }
    })
  };

  changeFilter = (filter, value) => {
    this.setState(state => ({...state, [filter]: value}));
  };

  dateConverter = date => {
    const year = date.getFullYear();
    const month = date.getMonth() + 1;
    const day = date.getDate();

    return `${month}.${day}.${year}`;
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
