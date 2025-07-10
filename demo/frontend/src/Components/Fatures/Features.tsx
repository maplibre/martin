import { Parallax } from "react-scroll-parallax";

import martinFeatures from "../../config/features";

import Container from "./Container";
import Description from "./Description";
import Feature from "./Feature";
import tiles from "./martin_mobile.png";
import Title from "./Title";

const Features = () => (
  <Container>
    {martinFeatures.map((feature) => (
      <Feature key={feature.id}>
        <Parallax
          translateX={[0, 20]}
          translateY={[50, -50]}
          // slowerScrollRate
        >
          <Title>{feature.title}</Title>
        </Parallax>
        <Parallax
          translateX={[10, 0]}
          translateY={[50, -40]}
          // slowerScrollRate
        >
          <Description>{feature.description}</Description>
        </Parallax>
      </Feature>
    ))}
    <Feature>
      <Title>
        <img alt="tiles" src={tiles} />
      </Title>
      <Parallax
        translateX={[10, 0]}
        translateY={[50, -40]}
        // slowerScrollRate
      >
        <Description>Start building with Martin!</Description>
      </Parallax>
    </Feature>
  </Container>
);

export default Features;
