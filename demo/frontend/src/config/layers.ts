export default [
  {
    id: 'trips',
    maplibreLayer: {
      id: 'trips',
      type: 'fill-extrusion',
      source: 'trips_source',
      'source-layer': 'trips',
      layout: {
        visibility: 'visible'
      },
      paint: {
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips'],
          3, 50,
          6, 90,
          15, 150,
          45, 250,
          100, 300,
          200, 500,
          375, 600,
          500, 700,
          1200, 800,
          2500, 900,
          4000, 1100,
          8000, 1400,
          12000, 1700,
          20000, 2200,
          50000, 2700,
          250000, 4000,
          500000, 5500,
          1000000, 7000,
          2000000, 8500,
          3000000, 10000
        ],
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips'],
          3,
          '#fdd8ec',
          15,
          '#fcc0e7',
          80,
          '#fba8e7',
          200,
          '#fb8feb',
          500,
          '#fb76f6',
          2500,
          '#f05dfb',
          8000,
          '#db44fb',
          12000,
          '#c12bfb',
          20000,
          '#a211fc',
          50000,
          '#7c02f2',
          250000,
          '#7c02f2',
          500000,
          '#5901da',
          1000000,
          '#3a00c2',
          2000000,
          '#3a00c2',
          3000000,
          '#3a00c2'
        ],
        'fill-extrusion-opacity': 0.75
      }
    },
    title: 'Number of trips',
    description: 'Conducted from an area'
  },
  {
    id: 'trips_price',
    maplibreLayer: {
      id: 'trips_price',
      type: 'fill-extrusion',
      source: 'trips_source',
      'source-layer': 'trips',
      layout: {
        visibility: 'none'
      },
      paint: {
        'fill-extrusion-opacity': 0.75,
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips_price'],
          10,
          '#d6ffe9',
          12,
          '#95f1de',
          19,
          '#07e2e3',
          28,
          '#00d0f5',
          35,
          '#00bcff',
          37,
          '#00a4ff',
          45,
          '#0088ff',
          60,
          '#0062ff',
          80,
          '#5314ff'
        ],
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips_price'],
          7, 20,
          12, 300,
          15, 600,
          20, 900,
          28, 1300,
          35, 1800,
          37, 2300,
          45, 2800,
          60, 3300,
          80, 4000
        ]
      }
    },
    title: 'Price',
    description: 'Average prices of the trips from an area'
  },
  {
    id: 'trips_duration',
    maplibreLayer: {
      id: 'trips_duration',
      type: 'fill-extrusion',
      source: 'trips_source',
      'source-layer': 'trips',
      layout: {
        visibility: 'none'
      },
      paint: {
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips_duration'],
          5,
          20,
          8,
          150,
          11,
          300,
          14,
          500,
          17,
          800,
          23,
          1200,
          42,
          1500
        ],
        'fill-extrusion-opacity': 0.75,
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips_duration'],
          5,
          '#29d4ff',
          8,
          '#3ab0fd',
          11,
          '#4b8dfb',
          14,
          '#5c6afa',
          17,
          '#6d46f8',
          23,
          '#7e23f6',
          42,
          '#8f00f5'
        ]
      }
    },
    title: 'Travel Time',
    description: 'Average travel times from an area'
  }
];
