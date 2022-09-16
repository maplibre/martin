import React from 'react';

import maplibre from './maplibre-logo-big.svg';

import Container from './Container';
// import Description from './Description';

const Footer = () => (
  <Container>
    <a href='https://maplibre.org'>
      <img src={maplibre} alt='MapLibre' />
    </a>
  </Container>
);

export default Footer;
