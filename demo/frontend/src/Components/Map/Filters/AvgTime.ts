import styled from 'styled-components';

export default styled.div`
  padding: 5px;
  border-radius: 5px;

  font-size: 16px;
  font-weight: bold;

  color: ${({ isEnabled }) => (isEnabled ? '#DADFEE' : '#6C7495')};
  
  cursor: pointer;

  &:hover {
    background-color: #161626;
  }
`;
