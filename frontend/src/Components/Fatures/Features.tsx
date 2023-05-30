import React from 'react'
import { Parallax } from 'react-scroll-parallax'

import martinFeatures from '../../config/features'

import Container from './Container'
import Feature from './Feature'
import Title from './Title'
import Description from './Description'

import tiles from './martin_mobile.png'

const Features = () => (
  <Container>
    {martinFeatures.map((feature) => (
      <Feature key={feature.id}>
        <Parallax
          translateY={[50, -50]}
          translateX={[0, 20]}
          // slowerScrollRate
        >
          <Title>{feature.title}</Title>
        </Parallax>
        <Parallax
          translateY={[50, -40]}
          translateX={[10, 0]}
          // slowerScrollRate
        >
          <Description>{feature.description}</Description>
        </Parallax>
      </Feature>
    ))}
    <Feature>
      <Title>
        <img src={tiles} alt='tiles' />
      </Title>
      <Parallax
        translateY={[50, -40]}
        translateX={[10, 0]}
        // slowerScrollRate
      >
        <Description>Start building with Martin!</Description>
      </Parallax>
    </Feature>
  </Container>
)

export default Features
