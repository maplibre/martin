import React from 'react';

import martinFeatures from '../../config/features';

import Container from './Container';
import Feature from './Feature';
import Title from './Title';
import Description from './Description';

const Features = () => (
  <Container>
    {martinFeatures.map(feature => (
      <Feature key={feature.id}>
        <Title>
          {feature.title}
        </Title>
        <Description>
          {feature.description}
        </Description>
      </Feature>
    ))}
  </Container>
);

export default Features;
