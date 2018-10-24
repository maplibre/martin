import React from 'react';

import urbica from './urbica.svg';
import arrow from './arrow.svg';

import Container from './Container';
import Description from './Description';

const Footer = () => (
  <Container>
    <Description>
      MADE BY
    </Description>
    <a href='https://urbica.co'>
      <img src={urbica} alt='urbica' />
      <img src={arrow} alt='arrow' />
    </a>
  </Container>
);

export default Footer;
