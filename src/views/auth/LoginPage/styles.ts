import styled from 'styled-components'

export const LoginContent = styled.div`
  width: 100%;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 200px;
  background-color: #252423;

  .right {
    display: flex;
    flex-direction: column;
    gap: 20px;
    justify-content: center;
    align-items: center;

    p {
      color: #ffffff;
      font-size: 25px;
      font-family: 'Poppins', sans-serif;
      font-weight: 700;
    }

    .email {
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      position: relative;
      height: 50px;

      label {
        top: 0;
        left: 5px;
        font-size: 0.75rem;
        color: #ffffff;
        font-weight: 700;
        position: absolute;
        margin: 0 0 0 7px;
        padding: 0 3px;
        background: #252423;
        border-radius: 50px;
        width: fit-content;
      }

      input[type='text'] {
        padding: 11px 10px;
        font-size: 0.75rem;
        border: 2px #ffffff solid;
        border-radius: 5px;
        background: #363432;
        width: 250px;
        color: #ffffff;

        &:focus {
          outline: none;
        }
      }
    }
    .password {
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      position: relative;
      height: 50px;

      label {
        top: 0;
        left: 5px;
        font-size: 0.75rem;
        color: #ffffff;
        font-weight: 700;
        position: absolute;
        margin: 0 0 0 7px;
        padding: 0 3px;
        background: #252423;
        border-radius: 50px;
        width: fit-content;
      }

      input {
        padding: 11px 10px;
        font-size: 0.75rem;
        border: 2px #ffffff solid;
        border-radius: 5px;
        background: #363432;
        width: 250px;
        color: #ffffff;

        &:focus {
          outline: none;
        }
      }
    }
    .bottom {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 10px;

      a {
        color: #90a19d;
        font-family: 'Poppins', sans-serif;
        font-size: 12px;
      }
    }
  }
`
