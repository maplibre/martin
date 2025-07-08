import React from "react";
import GitHubButton from "../GitHubButton";
import DocsButton from "../GitHubButton/DocsButton";
import Container from "./Container";
import Title from "./Title";

const Development = () => (
  <Container>
    <Title>Start building with Martin!</Title>
    <GitHubButton /> <DocsButton />
  </Container>
);

export default Development;
