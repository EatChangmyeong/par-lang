# 박스

Par의 일반적인 [함수](./function.md)는 선형이므로, 일단 지역 변수에 대입하면 정확히 한 번 호출해야 한다. 그런데 함수를 리스트의 모든 원소에 대해 호출하려면 어떻게 해야 할까?

이때는 박스를 사용할 수 있다. 박스는 유예된 연산을 담고 있으며, 언박싱할 때마다 해당 연산의 인스턴스를 만들어 새로운 값을 획득할 수 있다.

연산의 결과값이 타입 `T`라고 할 때, 이 값을 박싱한 타입은 `box T`라고 적는다.

```par
type IntCalculation = box Int
type ReusableFunction = box [Int] String
```

모든 박스는 내용과 관계 없이 [공유 가능](../types_and_expressions.md#선형성)하다. 즉, 복사하거나, 원하는 만큼 언박싱하거나, 사용하지 않고 버려도 된다.

## 생성

박스 값은 식 앞에 `box`를 붙여 생성할 수 있다.

```par
module Main

import {
  @core/Int
  @core/List
  @core/String
}

def Sum : box Int = box Int.Range(0, 1_000_000)->List.Sum
```

`box`가 없었다면 이 식은 정수 백만 개의 합을 바로 구했을 것이다. 한편 `box`가 있는 이 식은 **유예된 연산**이 된다. 아직은 아무런 덧셈 연산도 일어나지 않았다!

박스 값에서는 본문 밖에서 정의된 지역 변수도 사용할 수 있다.

```par
dec MakeAdder : [Int] box [Int] Int
def MakeAdder = [amount] box [n] n + amount
```

이때 박스에서 `amount`를 *포착*한다. 즉, 박스 안에는 유예된 연산과 포착된 변수가 같이 보관되어 있다. 새 연산을 시작할 때마다 포착된 변수 역시 같이 복사된다.

이런 이유로 **박스에서 포착하는 모든 변수는 공유 변수여야 한다.** `amount`와 같은 정수나 다른 박스를 포착하는 것은 가능하다. 하지만 일반적인 선형 함수를 포착하면 그 함수를 여러 번 호출할 수 있게 되는 꼴이므로 불가능하다!

## 소멸

새 연산을 시작하려면 `.unbox`를 적용하면 된다.

```par
def Total : Int = Sum.unbox
```

이때 `box Int`에서 `Int`가 새로 생성된다. 일반적으로 `box T`가 주어졌을 때 `.unbox`를 하면 `T`가 된다. 이 과정을 박스를 *인스턴스화*한다고 한다.

위에서 보았던 합을 두 번 사용한다고 하자.

```par
dec Add : [Int, Int] Int
def Add = [x, y] x + y

def Twice : Int =
  let n = box Int.Range(0, 1_000_000)->List.Sum
  in Add(n.unbox, n.unbox)
```

정수 백만 개의 합을 구하는 횟수는 몇 번인가? 답은 **두 번**이다. `.unbox`를 할 때마다 새 연산이 시작되기 때문이다. 박스는 이전 연산의 결과를 기억하지 않는다.

합을 한 번만 구하려면, 언박싱한 결과 자체를 변수에 대입하면 된다.

```par
def Once : Int =
  let n = box Int.Range(0, 1_000_000)->List.Sum
  in let value = n.unbox
  in Add(value, value)
```

여기서는 한 번 연산한 정수 결과값을 공유하고 있다. 위에서는 박스 자체를 복사했다면, 지금은 결과값을 복사한다.

> Par에서는 언박싱을 한다고 해도 연산의 결과가 끝날 때까지 기다리지 않으며, 다른 값과 마찬가지로 이 연산은 결과값을 사용하는 코드와 동시에 실행된다.

`.unbox`를 생략하고 그냥 `Add(n, n)`이라고 써도 될까? 안 된다. **`box T`와 `T`는 별개의 타입이다.** `T`가 원래 공유 타입이라고 해도 `T`와 `box T`는 서로 서브타입 관계를 가지지 않는다. `Add`에서는 정수를 전달받기 때문에 값을 복사하면서 실수로 연산을 반복하는 일이 없다. 함수 자체에서 연산을 반복하고자 할 때는 `box Int`를 대신 받을 수 있다.

## 다회용 함수

이제 `MakeAdder`를 사용해 보자.

```par
def Explicit : Int =
  let addTen = MakeAdder(10)
  in Add(addTen.unbox(1), addTen.unbox(2))  // = 23
```

`.unbox`를 할 때마다 새로운 함수가 생성되고, 각각 한 번 호출된다. 하지만 호출할 때마다 `.unbox`를 붙여야 한다면 불편하기 때문에, 생략하고 호출하는 것도 가능하다.

```par
def Implicit : Int =
  let addTen = MakeAdder(10)
  in Add(addTen(1), addTen(2))  // = 23 (위와 같이)
```

**박스의 내용물에 연산을 적용하면 자동으로 언박싱 처리가 된다.** 위에서와 같이 박싱된 함수를 호출하는 것뿐만 아니라, 박싱된 [선택](./choice.md)에서 분지를 선택하거나, 박싱된 [분기](./either.md)를 `.case`로 매칭할 때도 동일하다.

박스를 인자로 전달하는 것은 내용물에 대한 연산이 아니므로 `Add(n, n)`은 잘못된 코드이다. 한편 `addTen(1)`은 박싱된 함수를 인스턴스화한 뒤 호출하므로 문제가 없다.

이제 이번 장을 시작하면서 언급했던 리스트 매핑 함수를 실제로 구현할 수 있다.

```par
dec Map : [<a> List<a>, <b> box [a] b] List<b>
def Map = [<a> list, <b> f] list.begin.case {
  .end! => .end!,
  .item(x) xs => .item(f(x)) xs.loop,
}

def NumberStrings = Map(Int.Range(1, 100), box [n] `#{n}`)
```

`f(x)`가 실행될 때마다 함수를 인스턴스화한 뒤 다음 원소를 인자로 하여 호출한다. 리스트가 끝났을 때는 `f`가 박스이기 때문에 사용하지 않고 버려도 된다.