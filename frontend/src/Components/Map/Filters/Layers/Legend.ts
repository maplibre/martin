import styled from 'styled-components';

export default styled.div`
  height: 4px;
  border-radius: 5px;
  margin-top: 6px;

  background:
    linear-gradient(to right,
      ${({ fromColor, toColor }) => `${fromColor}, ${toColor}`});
`;
