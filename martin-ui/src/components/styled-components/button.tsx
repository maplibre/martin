import styled from "styled-components";


const PRIMARY_COLOR = "rgb(53, 28, 131)";
const SECONDARY_COLOR = "rgb(115, 0, 255)";
export const Button = styled.button<{active: boolean}>`
  background: #141414;
  color: white;
  border: none;
  border-radius: 4px;
  font-size: 16px;
  cursor: pointer;
  &:hover {
    background: #333333;
  }
  &:disabled {
    background: #333333;
    color: #666666;
    cursor: not-allowed;
  }
  .active {
    background: #333333;
  }
  ${props => props.active ?
  `background: ${SECONDARY_COLOR}; color: white;
   &:hover {
    background: ${PRIMARY_COLOR};
   }
  ` :
  `background: #141414; color: white;`
  }
  width: 24px;
  height: 24px;
}
    
  }
`;