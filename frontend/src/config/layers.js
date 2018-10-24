export default [
  {
    id: 'trips',
    mapboxLayer: {
      id: 'trips',
      type: 'fill-extrusion',
      source: 'public.get_trips',
      'source-layer': 'trips',
      layout: {
        visibility: 'visible'
      },
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
    },
    title: 'Number of trips',
    description: 'Conducted from an area'
  },
  {
    id: 'trips_price',
    mapboxLayer: {
      id: 'trips_price',
      type: 'fill-extrusion',
      source: 'public.get_trips',
      'source-layer': 'trips',
      layout: {
        visibility: 'none'
      },
      paint: {
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips_price'],
          3,
          '#70ffd2',
          17.6,
          '#00e4e5',
          32.3,
          'hsl(194, 100%, 50%)',
          61.5,
          'hsl(202, 100%, 50%)',
          90.8,
          'hsl(212, 100%, 50%)',
          120,
          'hsl(229, 100%, 48%)'
        ],
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips_price'],
          3,
          10,
          17.6,
          100,
          32.3,
          200,
          61.5,
          300,
          90.8,
          400,
          120,
          600
        ]
      }
    },
    title: 'Price',
    description: 'Average prices of the trips from an area'
  },
  {
    id: 'trips_duration',
    mapboxLayer: {
      id: 'trips_duration',
      type: 'fill-extrusion',
      source: 'public.get_trips',
      'source-layer': 'trips',
      layout: {
        visibility: 'none'
      },
      paint: {
        'fill-extrusion-height': [
          'interpolate',
          ['linear'],
          ['get', 'trips_duration'],
          10,
          100,
          40,
          300,
          60,
          1500,
          85,
          3000,
          120,
          4000
        ],
        'fill-extrusion-color': [
          'interpolate',
          ['linear'],
          ['get', 'trips_duration'],
          20,
          'hsl(207, 100%, 68%)',
          40,
          'hsl(189, 100%, 57%)',
          60,
          'hsl(247, 100%, 69%)',
          90,
          'hsl(261, 100%, 58%)',
          120,
          'hsl(227, 100%, 72%)'
        ]
      }
    },
    title: 'Travel Time',
    description: 'Average travel times from an area'
  }
];
