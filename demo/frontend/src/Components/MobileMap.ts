import styled from 'styled-components';

export default styled.img`
  display: none;
  width: 100%;

  margin-top: -120px;
  margin-bottom: 50px;
  
  @media (max-width: 500px) {
    display: block;
  }
`;
